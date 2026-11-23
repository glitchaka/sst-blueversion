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

const TRACK_SHA256: [&str; 2] = [
    "ffecd8d5d7c96dfba062aedc02742a5fad1987f2367a1ef14bb71a7f1419013d",
    "e49c316532be0e8c235ceb08a208f52821c0e07479715e143346a3177972b516",
];
const ORIGINAL_NAMES: [&str; 2] = [
    "The Best Keygen Song Ever! [HQ] - Kinespojken.mp3",
    "Keygen Music By FFF - Foundeer.mp3",
];

fn verify_track_bytes(bytes: &[u8], index: usize) -> bool {
    use sha2::{Digest, Sha256};
    if bytes.len() > 10 * 1024 * 1024 || bytes.len() < 100_000 { return false; }
    let hash = Sha256::digest(bytes);
    let actual = hash.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    actual == TRACK_SHA256[index]
}

fn verified_directory(dir: &std::path::Path) -> bool {
    TRACK_NAMES.iter().enumerate().all(|(index, name)| {
        std::fs::read(dir.join(name)).is_ok_and(|bytes| verify_track_bytes(&bytes, index))
    })
}

fn read_zip_tracks(archive: &std::path::Path) -> Option<[Vec<u8>; 2]> {
    use std::io::Read;
    let file = std::fs::File::open(archive).ok()?;
    if file.metadata().ok()?.len() > 30 * 1024 * 1024 { return None; }
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut tracks = [Vec::new(), Vec::new()];
    for (index, name) in TRACK_NAMES.iter().enumerate() {
        let key = format!("data/.sst/media/keygen/{name}");
        let mut file = zip.by_name(&key).ok()?;
        if file.size() > 10 * 1024 * 1024 { return None; }
        file.take(10 * 1024 * 1024 + 1).read_to_end(&mut tracks[index]).ok()?;
        if !verify_track_bytes(&tracks[index], index) { return None; }
    }
    Some(tracks)
}

/// A complete SST installer supplies data/.sst/media/keygen directly. For existing
/// installations, recover the two *original user-provided* files from local
/// Downloads or from the soundtrack ZIP if present. Never execute anything,
/// fetch media from a network or ask the operator to create directories.
fn soundtrack_directory() -> Result<PathBuf> {
    if let Some(directory) = env::var_os("SST_KEYGEN_MUSIC_DIR") {
        let dir = PathBuf::from(directory);
        if verified_directory(&dir) { return Ok(dir); }
        bail!("keygen: SST_KEYGEN_MUSIC_DIR no contiene las dos pistas originales verificadas.");
    }
    let exe = env::current_exe().context("No se encuentra sst.exe")?;
    let root = exe.parent().context("No se encuentra la carpeta de SST")?;
    let installed = root.join("data").join(".sst").join("media").join("keygen");
    if verified_directory(&installed) { return Ok(installed); }

    let cache = env::var_os("LOCALAPPDATA").map(PathBuf::from)
        .unwrap_or_else(env::temp_dir).join("SST").join("data").join(".sst").join("media").join("keygen");
    if verified_directory(&cache) { return Ok(cache); }

    let Some(profile) = env::var_os("USERPROFILE") else {
        bail!("keygen: falta la banda sonora en la distribución SST.");
    };
    let downloads = PathBuf::from(profile).join("Downloads");
    let mut tracks = [Vec::<u8>::new(), Vec::<u8>::new()];
    let archive_paths = [
        downloads.join("sst-keygen-soundtrack.zip"),
        downloads.join("SST-Keygen-Autoinstalador.zip"),
    ];
    let from_archive = archive_paths.iter().find_map(|path| read_zip_tracks(path));
    if let Some(found) = from_archive {
        tracks = found;
    } else {
        for (index, name) in ORIGINAL_NAMES.iter().enumerate() {
            let Ok(bytes) = std::fs::read(downloads.join(name)) else {
                bail!("keygen: la distribución SST no contiene los dos MP3. El instalador debe incluirlos.");
            };
            if !verify_track_bytes(&bytes, index) {
                bail!("keygen: la pista {name} no corresponde al archivo original.");
            }
            tracks[index] = bytes;
        }
    }
    std::fs::create_dir_all(&cache).context("No se pudo preparar el caché de audio de SST")?;
    for (index, name) in TRACK_NAMES.iter().enumerate() {
        let path = cache.join(name);
        let temporary = cache.join(format!("{name}.tmp"));
        std::fs::write(&temporary, &tracks[index])
            .with_context(|| format!("No se pudo preparar {name}"))?;
        std::fs::rename(&temporary, &path)
            .with_context(|| format!("No se pudo instalar automáticamente {name}"))?;
    }
    if !verified_directory(&cache) {
        bail!("keygen: no se pudo verificar la instalación automática de la banda sonora.");
    }
    Ok(cache)
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
                bail!("keygen: la banda sonora no está instalada: {}", track.display());
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
