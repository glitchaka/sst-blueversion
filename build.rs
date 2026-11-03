use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const FONT_URL: &str =
    "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/v3.3.0/patched-fonts/JetBrainsMono/Ligatures/Regular/JetBrainsMonoNerdFontMono-Regular.ttf";
const FONT_NAME: &str = "JetBrainsMonoNerdFontMono-Regular.ttf";
const HELIX_VERSION: &str = "25.07.1";
const HELIX_ARCHIVE_NAME: &str = "helix-25.07.1-x86_64-windows.zip";
const HELIX_URL: &str =
    "https://github.com/helix-editor/helix/releases/download/25.07.1/helix-25.07.1-x86_64-windows.zip";
const SPELL_AFF_URL: &str =
    "https://raw.githubusercontent.com/wooorm/dictionaries/8cfea406b505e4d7df52d5a19bce525df98c54ab/dictionaries/es-CL/index.aff";
const SPELL_DIC_URL: &str =
    "https://raw.githubusercontent.com/wooorm/dictionaries/8cfea406b505e4d7df52d5a19bce525df98c54ab/dictionaries/es-CL/index.dic";
const SPELL_LICENSE_URL: &str =
    "https://raw.githubusercontent.com/wooorm/dictionaries/8cfea406b505e4d7df52d5a19bce525df98c54ab/dictionaries/es-CL/license";
const IEEE_MAL_URL: &str = "https://standards-oui.ieee.org/oui/oui.csv";
const IEEE_MAM_URL: &str = "https://standards-oui.ieee.org/oui28/mam.csv";
const IEEE_MAS_URL: &str = "https://standards-oui.ieee.org/oui36/oui36.csv";

fn main() {
    println!("cargo:rerun-if-env-changed=SST_NERD_FONT_FILE");
    println!("cargo:rerun-if-env-changed=SST_HELIX_ARCHIVE");
    println!("cargo:rerun-if-env-changed=SST_IEEE_MAL_CSV");
    println!("cargo:rerun-if-env-changed=SST_IEEE_MAM_CSV");
    println!("cargo:rerun-if-env-changed=SST_IEEE_MAS_CSV");
    println!("cargo:rerun-if-changed=assets/shell-shock-mascot.svg");
    println!("cargo:rerun-if-changed=assets/sst-icon.ico");
    println!("cargo:rerun-if-changed=assets/sst-icon.png");
    println!("cargo:rerun-if-changed=bg");
    println!("cargo:rerun-if-changed=guide");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR no definido"));
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR no definido"),
    );
    let cache_dir = build_asset_cache_dir(&manifest_dir);
    fs::create_dir_all(&cache_dir)
        .expect("No se pudo crear el caché compartido de assets de compilación");

    sync_background_assets(&manifest_dir, &out_dir);
    prepare_guide_assets(&manifest_dir, &out_dir);
    ensure_nerd_font(&cache_dir, &out_dir);
    ensure_spell_dictionary(&cache_dir, &out_dir);
    ensure_ieee_registry(&cache_dir, &out_dir);

    #[cfg(windows)]
    ensure_helix_archive(&cache_dir, &out_dir);

    #[cfg(windows)]
    {
        let icon = manifest_dir.join("assets").join("sst-icon.ico");

        let mut resource = winres::WindowsResource::new();
        resource.set_icon(
            icon.to_str()
                .expect("La ruta del icono de Shell Shock Tool no es UTF-8"),
        );
        resource
            .compile()
            .expect("No se pudo incrustar el icono de Shell Shock Tool");
    }
}

fn build_asset_cache_dir(manifest_dir: &Path) -> PathBuf {
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                manifest_dir.join(path)
            }
        })
        .unwrap_or_else(|| manifest_dir.join("target"));
    target_dir.join("sst-build-assets")
}


fn prepare_guide_assets(manifest_dir: &Path, out_dir: &Path) {
    // Source of truth: repository's guide/ directory, not live HTTP.
    // Generate an embedded recovery copy so binary-only updates also work offline.
    let source = manifest_dir.join("guide");
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("No se pudo determinar target/<profile> desde OUT_DIR");
    let destination = profile_dir.join("guide");
    fs::create_dir_all(&destination).expect("No se pudo crear guide portable");

    let mut files = fs::read_dir(&source)
        .expect("Falta la carpeta local guide/ en el repositorio")
        .map(|entry| entry.expect("Entrada inválida de guide"))
        .filter(|entry| entry.path().is_file())
        .collect::<Vec<_>>();
    files.sort_by_key(|entry| entry.file_name());
    assert!(
        files.iter().any(|entry| entry.file_name() == "guide-index.json"),
        "Falta guide/guide-index.json"
    );

    let mut generated = String::from("pub(crate) const GUIDE_ASSETS: &[(&str, &[u8])] = &[\n");
    let mut digest = 0xcbf29ce484222325_u64;
    for entry in files {
        let name = entry.file_name();
        let filename = name.to_str().expect("Nombre de tutorial no UTF-8");
        assert!(
            filename.bytes().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_' | b'.')),
            "Nombre inválido de tutorial: {filename}"
        );
        let source_file = entry.path();
        let bytes = fs::read(&source_file).expect("No se pudo leer un tutorial");
        for byte in filename.bytes().chain(bytes.iter().copied()) {
            digest ^= byte as u64;
            digest = digest.wrapping_mul(0x100000001b3);
        }
        fs::copy(&source_file, destination.join(filename))
            .expect("No se pudo copiar la guía al directorio del ejecutable");
        generated.push_str(&format!(
            "    ({filename:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/guide/\", {filename:?}))),\n"
        ));
    }
    generated.push_str("];\n");
    generated.push_str(&format!(
        "pub(crate) const GUIDE_SNAPSHOT_ID: &str = \"{digest:016x}\";\n"
    ));
    fs::write(out_dir.join("guide_assets.rs"), generated)
        .expect("No se pudo generar el manifiesto de guía embebida");
}

fn sync_background_assets(manifest_dir: &Path, out_dir: &Path) {
    let source = manifest_dir.join("bg");
    if !source.is_dir() {
        return;
    }

    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .expect("No se pudo determinar target/<profile> desde OUT_DIR");
    let destination = profile_dir.join("bg");
    fs::create_dir_all(&destination)
        .expect("No se pudo crear la carpeta bg junto al ejecutable de SST");

    for entry in fs::read_dir(&source).expect("No se pudo leer la carpeta bg del repositorio") {
        let entry = entry.expect("No se pudo leer una entrada de bg");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        fs::copy(&path, destination.join(entry.file_name())).unwrap_or_else(|error| {
            panic!(
                "No se pudo copiar {} al bg portable: {error}",
                path.display()
            )
        });
    }
}

fn curl_program() -> PathBuf {
    #[cfg(windows)]
    {
        let root = env::var_os("SystemRoot")
            .or_else(|| env::var_os("WINDIR"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let path = root.join("System32").join("curl.exe");
        if !path.is_file() {
            panic!("No se encontró curl.exe del sistema en {}", path.display());
        }
        path
    }

    #[cfg(not(windows))]
    {
        PathBuf::from("curl")
    }
}

fn download_to_cache(url: &str, destination: &Path, description: &str) {
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("asset");
    let temporary = destination.with_file_name(format!("{name}.part"));
    let _ = fs::remove_file(&temporary);

    let status = Command::new(curl_program())
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--retry",
            "3",
            "--retry-all-errors",
            "--connect-timeout",
            "10",
            "--max-time",
            "180",
            url,
            "-o",
        ])
        .arg(&temporary)
        .status()
        .unwrap_or_else(|error| panic!("No se pudo ejecutar curl para {description}: {error}"));

    if !status.success() {
        let _ = fs::remove_file(&temporary);
        panic!("No se pudo descargar {description} desde {url}");
    }

    if destination.exists() {
        fs::remove_file(destination)
            .unwrap_or_else(|error| panic!("No se pudo reemplazar {description}: {error}"));
    }
    fs::rename(&temporary, destination)
        .unwrap_or_else(|error| panic!("No se pudo activar {description} descargado: {error}"));
}

fn validate_asset(path: &Path, minimum: u64, description: &str) {
    let metadata = fs::metadata(path)
        .unwrap_or_else(|_| panic!("Falta {description} en {}", path.display()));
    if metadata.len() < minimum {
        panic!("{description} parece incompleto");
    }
}

fn materialize_cached(cache_path: &Path, out_dir: &Path, name: &str, description: &str) {
    let destination = out_dir.join(name);
    fs::copy(cache_path, &destination).unwrap_or_else(|error| {
        panic!(
            "No se pudo copiar {description} desde el caché compartido a {}: {error}",
            destination.display()
        )
    });
}

fn ensure_nerd_font(cache_dir: &Path, out_dir: &Path) {
    let cache = cache_dir.join(FONT_NAME);

    if let Some(source) = env::var_os("SST_NERD_FONT_FILE") {
        fs::copy(Path::new(&source), &cache).expect("No se pudo copiar SST_NERD_FONT_FILE");
    } else if !cache.is_file() {
        download_to_cache(FONT_URL, &cache, "la Nerd Font");
    }

    validate_asset(&cache, 100_000, "La Nerd Font embebida");
    materialize_cached(&cache, out_dir, FONT_NAME, "la Nerd Font");
}

fn ensure_spell_dictionary(cache_dir: &Path, out_dir: &Path) {
    for (name, url, minimum) in [
        ("helix-sst-es-CL.aff", SPELL_AFF_URL, 50_000u64),
        ("helix-sst-es-CL.dic", SPELL_DIC_URL, 200_000u64),
        ("helix-sst-es-CL.LICENSE", SPELL_LICENSE_URL, 500u64),
    ] {
        let cache = cache_dir.join(name);
        if !cache.is_file() {
            download_to_cache(url, &cache, &format!("el diccionario ortográfico {name}"));
        }
        validate_asset(&cache, minimum, &format!("El archivo de diccionario {name}"));
        materialize_cached(&cache, out_dir, name, &format!("el diccionario {name}"));
    }
}

fn ensure_ieee_registry(cache_dir: &Path, out_dir: &Path) {
    for (name, url, override_var) in [
        ("ieee-ma-l.csv", IEEE_MAL_URL, "SST_IEEE_MAL_CSV"),
        ("ieee-ma-m.csv", IEEE_MAM_URL, "SST_IEEE_MAM_CSV"),
        ("ieee-ma-s.csv", IEEE_MAS_URL, "SST_IEEE_MAS_CSV"),
    ] {
        let cache = cache_dir.join(name);

        if let Some(source) = env::var_os(override_var) {
            fs::copy(Path::new(&source), &cache)
                .unwrap_or_else(|_| panic!("No se pudo copiar {override_var}"));
        } else if !cache.is_file() {
            download_to_cache(url, &cache, &format!("el registro IEEE {name}"));
        }

        validate_asset(&cache, 10_000, &format!("El registro IEEE {name}"));
        materialize_cached(&cache, out_dir, name, &format!("el registro IEEE {name}"));
    }
}

#[cfg(windows)]
fn ensure_helix_archive(cache_dir: &Path, out_dir: &Path) {
    let cache = cache_dir.join(HELIX_ARCHIVE_NAME);

    if let Some(source) = env::var_os("SST_HELIX_ARCHIVE") {
        fs::copy(Path::new(&source), &cache).expect("No se pudo copiar SST_HELIX_ARCHIVE");
    } else if !cache.is_file() {
        download_to_cache(HELIX_URL, &cache, &format!("Helix {HELIX_VERSION}"));
    }

    validate_asset(&cache, 1_000_000, "El paquete de Helix embebido");
    materialize_cached(&cache, out_dir, HELIX_ARCHIVE_NAME, "el paquete de Helix");
}
