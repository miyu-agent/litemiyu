use super::{clean, detect, fallback, linux, macos, os_name};

#[test]
fn linux_names_the_distribution_and_its_version() {
    let arch = "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\nBUILD_ID=rolling\n";
    assert_eq!(
        linux(arch).as_deref(),
        Some("Arch Linux"),
        "滚动更新的没有版本号"
    );
    let ubuntu = "PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nNAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nVERSION=\"24.04.1 LTS (Noble Numbat)\"\n";
    assert_eq!(linux(ubuntu).as_deref(), Some("Ubuntu 24.04"), "点版本不写");
    let fedora = "NAME='Fedora Linux'\nVERSION_ID=43\n";
    assert_eq!(linux(fedora).as_deref(), Some("Fedora Linux 43"));
    assert_eq!(linux("ID=arch\n"), None, "没有 NAME 的认不出");
    assert_eq!(linux("NAME=\"\"\n"), None);
    let pretty_only =
        "PRETTY_NAME=\"Debian GNU/Linux 13\"\nNAME=\"Debian GNU/Linux\"\nVERSION_ID=\"13\"\n";
    assert_eq!(
        linux(pretty_only).as_deref(),
        Some("Debian GNU/Linux 13"),
        "PRETTY_NAME 不当成 NAME"
    );
}

#[test]
fn a_hostile_os_release_stays_one_short_line() {
    let long = format!("NAME=\"{}\"\n", "x".repeat(200));
    assert_eq!(linux(&long).unwrap().chars().count(), 64);
    assert_eq!(clean("Arch\tLinux\u{7}  rolling"), "Arch Linux rolling");
    assert_eq!(
        linux("NAME=\"say \\\"hi\\\"\"\n").as_deref(),
        Some("say \"hi\"")
    );
}

#[test]
fn this_machine_always_has_a_name() {
    let name = os_name();
    assert!(!name.is_empty() && !name.contains('\n'), "{name:?}");
    assert_eq!(os_name(), name, "认一次，之后照它");
    assert_eq!(detect(), name);
}

/// macOS（LM-5）：照 `SystemVersion.plist` 写大版本；认不出的照编译时的系统写名字。
#[test]
fn macos_names_its_major_version_and_others_fall_back_to_the_system() {
    let plist = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\
                 \t<key>ProductName</key>\n\t<string>macOS</string>\n\
                 \t<key>ProductVersion</key>\n\t<string>15.1.1</string>\n</dict>\n</plist>\n";
    assert_eq!(macos(plist).as_deref(), Some("macOS 15"));
    assert_eq!(macos("<plist></plist>"), None, "没有版本的认不出");
    assert_eq!(
        macos("<key>ProductVersion</key><string>x</string>"),
        None,
        "不是数的不写"
    );
    assert_eq!(fallback("linux"), "Linux");
    assert_eq!(fallback("freebsd"), "FreeBSD");
    assert_eq!(fallback("openbsd"), "OpenBSD");
}
