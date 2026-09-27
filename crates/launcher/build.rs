use std::path::{Path, PathBuf};

/// Icon group resource IDs. Keep in sync with `src/windows/app_icon.rs`.
const ICON_GROUPS: [(u16, &str); 3] = [
    (1, "core.ico"),
    (2, "tray-white.ico"),
    (3, "tray-black.ico"),
];
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const MOVEABLE_DISCARDABLE: u16 = 0x1010;
const MOVEABLE_PURE_DISCARDABLE: u16 = 0x1030;
const LANGUAGE_NEUTRAL: u16 = 0;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let package =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo package directory"));
    let manifest = package.join("core.manifest");
    println!("cargo:rustc-link-arg-bin=core-v2=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bin=core-v2=/MANIFESTINPUT:{}",
        manifest.display()
    );
    println!("cargo:rerun-if-changed=core.manifest");

    let assets = package.join("assets");
    let resources = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"))
        .join("core-icons.res");
    std::fs::write(&resources, icon_resources(&assets)).expect("write icon resources");
    // link.exe converts .res inputs itself, so no resource compiler is required.
    println!("cargo:rustc-link-arg-bin=core-v2={}", resources.display());
    for (_, file) in ICON_GROUPS {
        println!("cargo:rerun-if-changed=assets/{file}");
    }
}

/// Builds a Win32 .res file: one RT_ICON per image and one RT_GROUP_ICON per .ico file.
fn icon_resources(assets: &Path) -> Vec<u8> {
    let mut output = Vec::new();
    // A .res file starts with an empty 32-byte entry that identifies the format.
    write_entry(&mut output, 0, 0, 0, &[]);
    let mut next_image = 1_u16;
    for (group, file) in ICON_GROUPS {
        let path = assets.join(file);
        let icon =
            std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let count = read_u16(&icon, 4);
        assert!(
            read_u16(&icon, 2) == 1 && count > 0,
            "{} is not an icon file",
            path.display()
        );
        let mut directory = icon[..6].to_vec();
        for index in 0..usize::from(count) {
            let entry = &icon[6 + index * 16..22 + index * 16];
            let size = read_u32(entry, 8) as usize;
            let offset = read_u32(entry, 12) as usize;
            let image = icon
                .get(offset..offset + size)
                .unwrap_or_else(|| panic!("{} has a truncated image", path.display()));
            write_entry(
                &mut output,
                RT_ICON,
                next_image,
                MOVEABLE_DISCARDABLE,
                image,
            );
            // Group entries match .ico entries except the 32-bit offset becomes a 16-bit ID.
            directory.extend_from_slice(&entry[..12]);
            directory.extend_from_slice(&next_image.to_le_bytes());
            next_image += 1;
        }
        write_entry(
            &mut output,
            RT_GROUP_ICON,
            group,
            MOVEABLE_PURE_DISCARDABLE,
            &directory,
        );
    }
    output
}

fn write_entry(output: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    const HEADER_SIZE: u32 = 32;
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(&HEADER_SIZE.to_le_bytes());
    for ordinal in [kind, name] {
        output.extend_from_slice(&0xFFFF_u16.to_le_bytes());
        output.extend_from_slice(&ordinal.to_le_bytes());
    }
    output.extend_from_slice(&0_u32.to_le_bytes()); // DataVersion
    output.extend_from_slice(&flags.to_le_bytes());
    output.extend_from_slice(&LANGUAGE_NEUTRAL.to_le_bytes());
    output.extend_from_slice(&0_u32.to_le_bytes()); // Version
    output.extend_from_slice(&0_u32.to_le_bytes()); // Characteristics
    output.extend_from_slice(data);
    output.resize(output.len().next_multiple_of(4), 0);
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}
