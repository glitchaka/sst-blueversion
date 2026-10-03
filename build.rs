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
const SPELL_DICT_REV: &str = "8cfea406b505e4d7df52d5a19bce525df98c54ab";
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
    println!("cargo:rerun-if-changed=assets/sst-neofetch.png");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR no definido"));
    ensure_nerd_font(&out_dir);
    ensure_spell_dictionary(&out_dir);
    ensure_ieee_registry(&out_dir);

    #[cfg(windows)]
    ensure_helix_archive(&out_dir);

    #[cfg(windows)]
    {
        let manifest_dir = PathBuf::from(
            env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR no definido"),
        );
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
        return path;
    }

    #[cfg(not(windows))]
    {
        PathBuf::from("curl")
    }
}

fn ensure_nerd_font(out_dir: &Path) {
    let destination = out_dir.join(FONT_NAME);

    if let Some(source) = env::var_os("SST_NERD_FONT_FILE") {
        fs::copy(Path::new(&source), &destination)
            .expect("No se pudo copiar SST_NERD_FONT_FILE");
    } else if !destination.is_file() {
        let status = Command::new(curl_program())
            .args([
                "-L",
                "--fail",
                "--silent",
                "--show-error",
                FONT_URL,
                "-o",
            ])
            .arg(&destination)
            .status()
            .expect("No se pudo ejecutar curl para obtener la Nerd Font durante la compilación");

        if !status.success() {
            panic!(
                "No se pudo obtener la Nerd Font. Compile con Internet o defina SST_NERD_FONT_FILE apuntando a JetBrainsMono Nerd Font Mono."
            );
        }
    }

    let metadata = fs::metadata(&destination)
        .expect("La Nerd Font embebida no existe");
    if metadata.len() < 100_000 {
        panic!("La Nerd Font embebida parece incompleta");
    }
}


fn ensure_spell_dictionary(out_dir: &Path) {
    for (name, url, minimum) in [
        ("helix-sst-es-CL.aff", SPELL_AFF_URL, 50_000u64),
        ("helix-sst-es-CL.dic", SPELL_DIC_URL, 200_000u64),
        ("helix-sst-es-CL.LICENSE", SPELL_LICENSE_URL, 500u64),
    ] {
        let destination = out_dir.join(name);
        if !destination.is_file() {
            let status = Command::new(curl_program())
                .args(["-L", "--fail", "--silent", "--show-error", url, "-o"])
                .arg(&destination)
                .status()
                .expect("No se pudo ejecutar curl para obtener el diccionario de Helix-SST");
            if !status.success() {
                panic!(
                    "No se pudo obtener el diccionario ortográfico es-CL de Helix-SST (rev {SPELL_DICT_REV})."
                );
            }
        }
        let metadata = fs::metadata(&destination)
            .expect("Falta un archivo del diccionario ortográfico de Helix-SST");
        if metadata.len() < minimum {
            panic!("El archivo de diccionario {name} parece incompleto");
        }
    }
}

fn ensure_ieee_registry(out_dir: &Path) {
    for (name, url, override_var) in [
        ("ieee-ma-l.csv", IEEE_MAL_URL, "SST_IEEE_MAL_CSV"),
        ("ieee-ma-m.csv", IEEE_MAM_URL, "SST_IEEE_MAM_CSV"),
        ("ieee-ma-s.csv", IEEE_MAS_URL, "SST_IEEE_MAS_CSV"),
    ] {
        let destination = out_dir.join(name);

        if let Some(source) = env::var_os(override_var) {
            fs::copy(Path::new(&source), &destination)
                .unwrap_or_else(|_| panic!("No se pudo copiar {override_var}"));
        } else if !destination.is_file() {
            let status = Command::new(curl_program())
                .args(["-L", "--fail", "--silent", "--show-error", url, "-o"])
                .arg(&destination)
                .status()
                .expect("No se pudo ejecutar curl para obtener el registro IEEE de MAC");

            if !status.success() {
                panic!(
                    "No se pudo obtener {url}. Compile con Internet o defina {override_var} con el CSV oficial de IEEE."
                );
            }
        }

        let metadata = fs::metadata(&destination)
            .unwrap_or_else(|_| panic!("Falta el registro IEEE {name}"));
        if metadata.len() < 10_000 {
            panic!("El registro IEEE {name} parece incompleto");
        }
    }
}

#[cfg(windows)]
fn ensure_helix_archive(out_dir: &Path) {
    let destination = out_dir.join(HELIX_ARCHIVE_NAME);

    if let Some(source) = env::var_os("SST_HELIX_ARCHIVE") {
        fs::copy(Path::new(&source), &destination)
            .expect("No se pudo copiar SST_HELIX_ARCHIVE");
    } else if !destination.is_file() {
        let status = Command::new(curl_program())
            .args([
                "-L",
                "--fail",
                "--silent",
                "--show-error",
                HELIX_URL,
                "-o",
            ])
            .arg(&destination)
            .status()
            .expect("No se pudo ejecutar curl para obtener Helix durante la compilación");

        if !status.success() {
            panic!(
                "No se pudo obtener Helix {HELIX_VERSION}. Compile con Internet o defina SST_HELIX_ARCHIVE apuntando al ZIP oficial."
            );
        }
    }

    let metadata = fs::metadata(&destination)
        .expect("El paquete de Helix embebido no existe");
    if metadata.len() < 1_000_000 {
        panic!("El paquete de Helix embebido parece incompleto");
    }
}
