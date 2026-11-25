//! Onset envelope obtained from the original decoded MP3, never a guessed BPM.
use std::path::Path;
use anyhow::{Context,Result};
use symphonia::core::{audio::SampleBuffer,codecs::DecoderOptions,errors::Error,
    formats::FormatOptions,io::MediaSourceStream,meta::MetadataOptions,probe::Hint};

const HOP_SECONDS:f32=0.01;

#[derive(Clone,Copy,Debug,Default)]
pub(crate) struct VisualBeat {
    pub seconds:f32,
    pub pulse:f32,
    pub beat_count:usize,
    pub beat_phase:f32,
}

#[derive(Default)]
pub(super) struct BeatMap {
    // Each tuple holds the decoded low-frequency and transient energy over 10 ms.
    envelope:Vec<(f32,f32)>,
    onsets:Vec<usize>,
}
#[derive(Default)]
struct Analyzer {
    low:f32, previous:f32, low_sum:f32, high_sum:f32, n:usize,
    energy:Vec<(f32,f32)>,
}
impl Analyzer {
    fn push(&mut self, sample:f32, rate:u32) {
        let alpha=1.0-(-std::f32::consts::TAU*145.0/rate as f32).exp();
        self.low+=alpha*(sample-self.low);
        let high=sample-self.previous;
        self.previous=sample;
        self.low_sum+=self.low*self.low;
        self.high_sum+=high*high;
        self.n+=1;
        if self.n>=(rate/100).max(1) as usize {
            self.energy.push(((self.low_sum/self.n as f32).sqrt(),
                (self.high_sum/self.n as f32).sqrt()));
            self.low_sum=0.0;self.high_sum=0.0;self.n=0;
        }
    }
    fn finish(self)->BeatMap {BeatMap::from_energy(self.energy)}
}
impl BeatMap {
    fn from_energy(energy:Vec<(f32,f32)>)->Self {
        if energy.is_empty(){return Self::default();}
        let mut lo:Vec<f32>=energy.iter().map(|x|x.0).collect();
        let mut hi:Vec<f32>=energy.iter().map(|x|x.1).collect();
        lo.sort_by(f32::total_cmp);hi.sort_by(f32::total_cmp);
        let ls=lo[(lo.len()-1)*9/10].max(0.0001);
        let hs=hi[(hi.len()-1)*9/10].max(0.0001);
        let envelope:Vec<_>=energy.into_iter()
            .map(|(l,h)|((l/ls).clamp(0.0,2.0),(h/hs).clamp(0.0,2.0))).collect();
        // Positive spectral-envelope flux, with a local adaptive threshold.
        // A 140 ms refractory interval separates percussion onsets.
        let mut flux=vec![0.0_f32;envelope.len()];
        for i in 2..envelope.len(){
            let (l,h)=envelope[i];let (p,q)=envelope[i-2];
            flux[i]=0.7*(l-p).max(0.0)+0.3*(h-q).max(0.0);
        }
        let mut onsets=Vec::new();
        for i in 3..flux.len().saturating_sub(2) {
            let prior=&flux[i.saturating_sub(40)..i];
            let avg=prior.iter().sum::<f32>()/prior.len() as f32;
            if flux[i]>0.10_f32.max(avg*1.7) && flux[i]>=flux[i-1] && flux[i]>=flux[i+1]
                && onsets.last().is_none_or(|last|i-*last>=14) {
                onsets.push(i);
            }
        }
        Self{envelope,onsets}
    }
    pub(super) fn read(path:&Path)->Result<Self> {
        let stream=MediaSourceStream::new(Box::new(std::fs::File::open(path)?),Default::default());
        let mut hint=Hint::new();hint.with_extension("mp3");
        let mut format=symphonia::default::get_probe()
            .format(&hint,stream,&FormatOptions::default(),&MetadataOptions::default())
            .context("keygen: no se pudo analizar el MP3 original")?.format;
        let track=format.default_track().context("keygen: MP3 sin pista de audio")?;
        let id=track.id;
        let mut decoder=symphonia::default::get_codecs()
            .make(&track.codec_params,&DecoderOptions::default())?;
        let mut analysis=Analyzer::default();
        let mut buffer:Option<SampleBuffer<f32>>=None;
        loop {
            let packet=match format.next_packet(){
                Ok(p)=>p,
                Err(Error::IoError(e)) if e.kind()==std::io::ErrorKind::UnexpectedEof=>break,
                Err(e)=>return Err(e.into()),
            };
            if packet.track_id()!=id{continue;}
            let decoded=match decoder.decode(&packet){
                Ok(d)=>d,
                Err(Error::DecodeError(_))=>continue,
                Err(e)=>return Err(e.into()),
            };
            let spec=*decoded.spec();
            let b=buffer.get_or_insert_with(||SampleBuffer::new(decoded.capacity() as u64,spec));
            b.copy_interleaved_ref(decoded);
            for frame in b.samples().chunks_exact(spec.channels.count()){
                let mono=frame.iter().sum::<f32>()/frame.len() as f32;
                analysis.push(mono,spec.rate);
            }
        }
        let map=analysis.finish();
        anyhow::ensure!(!map.envelope.is_empty(),"keygen: no se decodificaron muestras del MP3");
        Ok(map)
    }
    pub(super) fn at(&self,seconds:f32)->VisualBeat {
        let seconds=seconds.max(0.0);
        let t=seconds/HOP_SECONDS;
        let index=t.floor() as usize;
        let count=self.onsets.partition_point(|&v|v<=index);
        let last=count.checked_sub(1).map(|i|self.onsets[i] as f32);
        let next=self.onsets.get(count).map(|&v|v as f32);
        let phase=match(last,next){
            (Some(a),Some(b))=>((t-a)/(b-a)).clamp(0.0,1.0),
            (None,Some(b))=>(t/b.max(1.0)).clamp(0.0,1.0),
            _=>0.0,
        };
        let kick=last.map_or(0.0,|v| (-(t-v).max(0.0)*HOP_SECONDS/0.13).exp());
        let energy=self.envelope.get(index).map_or(0.0,|(lo,hi)|0.75*lo+0.25*hi);
        VisualBeat{seconds,pulse:(0.75*kick+0.25*energy).clamp(0.0,1.0),
            beat_count:count,beat_phase:phase}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pulse_follows_actual_onsets_and_not_frame_count(){
        let mut e=vec![(0.02,0.02);500];
        for at in [50,100,150,230,340]{
            for i in at..at+6{e[i]=(1.0,0.8);}
        }
        let map=BeatMap::from_energy(e);
        assert!(map.onsets.len()>=5);
        assert!(map.at(1.01).pulse>map.at(0.80).pulse);
        assert!(map.at(1.5).beat_count>map.at(1.2).beat_count);
        assert!(map.at(2.31).pulse>map.at(2.0).pulse);
    }
    #[test]
    fn original_mp3s_have_distinct_audio_timing_when_packaged(){
        let folder=Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("keygen");
        let paths=[folder.join("track01.mp3"),folder.join("track02.mp3")];
        if !paths.iter().all(|p|p.is_file()){return;}
        let one=BeatMap::read(&paths[0]).unwrap();
        let two=BeatMap::read(&paths[1]).unwrap();
        assert!(one.onsets.len()>10 && two.onsets.len()>10);
        assert_ne!(one.onsets,two.onsets);
    }
}
