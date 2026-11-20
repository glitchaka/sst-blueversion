//! Native Windows soundtrack for SST's keygen screen.
//! No shell commands, external media players or detached playback processes.
use std::{env, path::PathBuf};

use anyhow::{Context, Result, bail};

pub const TRACK_NAMES: [&str; 2] = ["track01.mp3", "track02.mp3"];

#[cfg(windows)]
#[link(name = "winmm")]
unsafe extern "system" {
    fn mciSendStringW(
        command: *const u16,
        output: *mut u16,
        output_len: u32,
        callback: *mut std::ffi::c_void,
    ) -> u32;
}

#[cfg(windows)]
fn mci(command: &str) -> Result<String> {
    use std::os::windows::ffi::OsStrExt;
    let wide = std::ffi::OsStr::new(command)
        .encode_wide().chain(std::iter::once(0)).collect::<Vec<_>>();
    let mut reply = [0u16; 256];
    let status = unsafe {
        mciSendStringW(wide.as_ptr(), reply.as_mut_ptr(),
            reply.len() as u32, std::ptr::null_mut())
    };
    if status != 0 {
        bail!("WinMM/MCI error {status} en el reproductor SST");
    }
    let end = reply.iter().position(|ch| *ch == 0).unwrap_or(reply.len());
    Ok(String::from_utf16_lossy(&reply[..end]))
}

fn soundtrack_directory() -> Result<PathBuf> {
    if let Some(directory) = env::var_os("SST_KEYGEN_MUSIC_DIR") {
        return Ok(PathBuf::from(directory));
    }
    let exe = env::current_exe().context("No se encuentra sst.exe")?;
    let root = exe.parent().context("No se encuentra la carpeta de SST")?;
    Ok(root.join("assets").join("keygen"))
}

/// Owns the MCI alias. Drop closes it synchronously, including on errors.
pub struct KeygenMusic {
    #[cfg(windows)]
    open: bool,
    index: usize,
    tracks: [PathBuf; 2],
    paused: bool,
}

impl KeygenMusic {
    pub fn new(first: usize) -> Result<Self> {
        let folder = soundtrack_directory()?;
        let tracks = [folder.join(TRACK_NAMES[0]), folder.join(TRACK_NAMES[1])];
        for track in &tracks {
            if !track.is_file() {
                bail!(
                    "keygen: falta {}. Copia los dos MP3 al directorio {}",
                    track.display(), folder.display()
                );
            }
        }
        let mut player = Self {
            #[cfg(windows)]
            open: false,
            index: first % 2,
            tracks,
            paused: false,
        };
        player.start(first % 2)?;
        Ok(player)
    }

    pub fn track_number(&self) -> usize { self.index + 1 }

    pub fn is_paused(&self) -> bool { self.paused }

    #[cfg(windows)]
    fn close(&mut self) {
        if self.open {
            let _ = mci("stop SSTKeygenAudio");
            let _ = mci("close SSTKeygenAudio");
            self.open = false;
        }
    }

    #[cfg(not(windows))]
    fn close(&mut self) {}

    fn start(&mut self, next: usize) -> Result<()> {
        self.close();
        self.index = next % self.tracks.len();
        self.paused = false;
        #[cfg(windows)]
        {
            let path = self.tracks[self.index]
                .canonicalize()
                .context("No se pudo resolver el MP3 para keygen")?;
            // Quoted Windows paths are supplied only by the SST asset directory.
            let path = path.to_string_lossy();
            if path.contains('"') { bail!("keygen: ruta MP3 inválida"); }
            mci(&format!("open \"{path}\" type mpegvideo alias SSTKeygenAudio"))?;
            self.open = true;
            if let Err(error) = mci("play SSTKeygenAudio from 0") {
                self.close();
                return Err(error);
            }
        }
        #[cfg(not(windows))]
        bail!("keygen: la reproducción de MP3 requiere Windows");
        #[cfg(windows)]
        Ok(())
    }

    pub fn select(&mut self, track: usize) -> Result<()> {
        self.start(track)
    }

    pub fn pause_resume(&mut self) -> Result<()> {
        #[cfg(windows)]
        {
            if self.paused {
                mci("resume SSTKeygenAudio")?;
            } else {
                mci("pause SSTKeygenAudio")?;
            }
            self.paused = !self.paused;
        }
        Ok(())
    }

    /// Playlist looping: when a track completes, automatically play the next.
    pub fn tick(&mut self) -> Result<()> {
        if self.paused { return Ok(()); }
        #[cfg(windows)]
        if mci("status SSTKeygenAudio mode")?.trim().eq_ignore_ascii_case("stopped") {
            self.start((self.index + 1) % self.tracks.len())?;
        }
        Ok(())
    }
}

impl Drop for KeygenMusic {
    fn drop(&mut self) { self.close(); }
}
