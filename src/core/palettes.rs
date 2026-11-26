//! Single registry of SST font schemes. Source swatches remain unmodified.
pub struct Palette {
    pub name: &'static str,
    pub swatches: &'static [u32],
    // Title, Command, Label, Detail, Value, Success, Warning, Failure, Optimal.
    pub roles: Option<[i8;9]>,
}
pub const PALETTES: &[Palette] = &[
    Palette { name: "gruvbox", swatches: &[], roles: None },
    Palette { name: "sunset", swatches: &[0x3D4D65, 0x032451, 0xCE2F25, 0xF94C1F, 0xFE8005, 0xFBBA76], roles: None },
    Palette { name: "golden", swatches: &[0xE3D473, 0xD72E2E, 0x0E5047, 0x22153C, 0x000F08], roles: None },
    Palette { name: "robot", swatches: &[0x88B8FF, 0x213878, 0xC1911B, 0xB3441A, 0x521E1A], roles: Some([2, 0, 1, 0, 0, -1, 2, 3, 0]) },
    Palette { name: "cyberpunk", swatches: &[0xFD8326, 0xF59192, 0xF4545E, 0xD32729, 0x6D2A4F, 0x25091C], roles: Some([0, 2, 4, 1, 1, -1, 0, 3, -1]) },
    Palette { name: "stellar", swatches: &[0x142030, 0xE9D8C8, 0x732553, 0xFF5C8D, 0x1E3442, 0x85A3B2], roles: Some([3, 5, 5, 1, 1, -1, -1, 3, 5]) },
    Palette { name: "aqua", swatches: &[0x78161E, 0xEF3E18, 0x94EEE3, 0x33C6BA, 0x2E5C58], roles: Some([3, 2, 4, 2, 2, 3, -1, 1, 2]) },
    Palette { name: "industrial", swatches: &[0x222A2A, 0x4FBBBC, 0xEC4624, 0xF39120, 0xE2DAC2], roles: Some([3, 1, 1, 4, 4, 1, 3, 2, 1]) },
    Palette { name: "oxidize", swatches: &[0x1D0F28, 0xE10109, 0xFF1F0D, 0xFF8C01, 0x0C7473], roles: Some([3, 2, 4, 3, 3, 4, 3, 1, 4]) },
    Palette { name: "starfield", swatches: &[0xD5DAD7, 0xD3A54F, 0xD35F3F, 0xB72B3C, 0x334A73, 0x1F2C41], roles: Some([1, 2, 4, 0, 0, -1, 1, 3, 4]) },
    Palette { name: "ronin-sunset", swatches: &[0xFDB563, 0xE67090, 0xB96682, 0x685A7D, 0x3B5D80], roles: Some([0, 1, 3, 0, 1, -1, 0, 2, 4]) },
    Palette { name: "palm-leaf", swatches: &[0x364023, 0x6A823E, 0x9C9F69, 0xC7A39B, 0xE6B1C4, 0xEFD4DD], roles: Some([2, 4, 3, 5, 5, 1, -1, -1, -1]) },
    Palette { name: "brown-coffee", swatches: &[0x461D3A, 0x502A50, 0x7E2A53, 0xBA71A2, 0xECD0EC], roles: Some([3, 2, 2, 4, 4, -1, -1, -1, -1]) },
    Palette { name: "blaze-orange", swatches: &[0xF06B04, 0xFB8728, 0xF5ECD2, 0x269FC7, 0x1570AC, 0xA10815], roles: Some([1, 0, 3, 2, 2, -1, 1, 5, 3]) },
    Palette { name: "japanese-violet", swatches: &[0x502A50, 0x7E2A53, 0xBA71A2, 0xE6B1C4, 0xEFD4DD], roles: Some([2, 3, 2, 4, 4, -1, -1, -1, -1]) },
    Palette { name: "curious-blue", swatches: &[0x269FC7, 0x1570AC, 0x364023, 0x6A823E, 0xC7A39B], roles: Some([0, 1, 4, 4, 0, 3, -1, -1, 0]) },
    Palette { name: "classic-rose", swatches: &[0xE6B1C4, 0xEFD4DD, 0xBA71A2, 0x7E2A53, 0xA10815], roles: Some([0, 2, 2, 1, 1, -1, -1, 4, -1]) },
    Palette { name: "mondongo", swatches: &[0x7B7A7A, 0xFFFF00, 0x00FFF7, 0xFF8B00, 0x8511FF, 0x008F39], roles: Some([1, 4, 0, 2, 2, 5, 3, -1, 2]) },
];
pub fn names()->impl Iterator<Item=&'static str> { PALETTES.iter().map(|p|p.name) }
pub fn get(name:&str)->Option<&'static Palette> { PALETTES.iter().find(|p|p.name==name) }
pub fn listing()->String {
    let mut out=String::new();
    for p in PALETTES {
        let swatches=p.swatches.iter().map(|c|format!("#{c:06X}")).collect::<Vec<_>>().join(" ");
        out.push_str(&format!("{}  {}\n",p.name.to_uppercase(),if swatches.is_empty(){"Original".to_owned()}else{swatches}));
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_is_unique_and_role_indices_are_valid() {
        assert_eq!(PALETTES.len(),18);
        for p in PALETTES {
            assert_eq!(names().filter(|n|*n==p.name).count(),1);
            if let Some(roles)=p.roles {
                assert!(roles.iter().all(|&i|i == -1 || (i >= 0 && (i as usize) < p.swatches.len())));
            }
        }
        assert_eq!(get("mondongo").unwrap().swatches,&[0x7B7A7A,0xFFFF00,0x00FFF7,0xFF8B00,0x8511FF,0x008F39]);
    }
}
