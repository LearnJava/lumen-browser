//! Дистрибутив Linux и команда, которой ставятся кодеки H.264/HEVC/AAC
//! (COMPAT-MEDIA-OSDEC-LINUX).
//!
//! Декодеры патентованных кодеков Lumen берёт у системы (GStreamer и его плагины,
//! ADR-035 п.3) и сам не ставит. Если их нет, плашка в браузере показывает
//! команду для дистрибутива пользователя; определение дистрибутива и сами
//! команды — чистые функции, проверяемые на любой ОС.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// Семейство дистрибутивов, для которого известна команда установки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinuxDistro {
    /// Debian, Ubuntu, Linux Mint, Pop!_OS… (`apt`).
    Debian,
    /// Fedora, RHEL-семейство (`dnf`, репозиторий RPM Fusion).
    Fedora,
    /// openSUSE (`zypper`, репозиторий Packman).
    Suse,
    /// Arch, Manjaro, EndeavourOS (`pacman`).
    Arch,
    /// Приложение запущено из Flatpak: кодеки идут расширением среды.
    Flatpak,
    /// Дистрибутив не распознан.
    Other,
}

impl LinuxDistro {
    /// Команда установки декодеров (GStreamer: base, bad — включая VA-API, libav).
    #[must_use]
    pub const fn install_command(self) -> &'static str {
        match self {
            Self::Debian => "sudo apt install gstreamer1.0-plugins-base gstreamer1.0-plugins-bad gstreamer1.0-libav",
            Self::Fedora => "sudo dnf install gstreamer1-plugins-base gstreamer1-plugins-bad-freeworld gstreamer1-libav",
            Self::Suse => "sudo zypper install --from packman gstreamer-plugins-libav gstreamer-plugins-bad",
            Self::Arch => "sudo pacman -S gst-plugins-base gst-plugins-bad gst-libav",
            Self::Flatpak => "flatpak install flathub org.freedesktop.Platform.ffmpeg-full",
            Self::Other => "Установите GStreamer 1.x с плагинами base, bad и libav из репозитория дистрибутива",
        }
    }

    /// Что должно быть настроено до команды (репозиторий с патентованными кодеками).
    #[must_use]
    pub const fn repository_note(self) -> &'static str {
        match self {
            Self::Debian | Self::Arch | Self::Flatpak | Self::Other => "",
            Self::Fedora => "Нужен подключённый репозиторий RPM Fusion Free.",
            Self::Suse => "Нужен подключённый репозиторий Packman.",
        }
    }
}

/// Идентификаторы дистрибутива из `/etc/os-release`: `ID` и слова `ID_LIKE`.
fn os_release_ids(os_release: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in os_release.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().trim_matches(['"', '\'']).to_ascii_lowercase();
        match key.trim() {
            "ID" => ids.insert(0, value),
            "ID_LIKE" => ids.extend(value.split_whitespace().map(str::to_owned)),
            _ => {}
        }
    }
    ids
}

/// Дистрибутив по содержимому `/etc/os-release`; `flatpak` — приложение внутри песочницы.
#[must_use]
pub fn detect(os_release: &str, flatpak: bool) -> LinuxDistro {
    if flatpak {
        return LinuxDistro::Flatpak;
    }
    for id in os_release_ids(os_release) {
        match id.as_str() {
            "debian" | "ubuntu" | "linuxmint" | "pop" | "raspbian" | "elementary" => return LinuxDistro::Debian,
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" => return LinuxDistro::Fedora,
            "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "suse" | "sles" => return LinuxDistro::Suse,
            "arch" | "manjaro" | "endeavouros" | "cachyos" => return LinuxDistro::Arch,
            _ => {}
        }
    }
    LinuxDistro::Other
}

/// Дистрибутив этой машины (читается один раз).
#[cfg(target_os = "linux")]
#[must_use]
pub fn current() -> LinuxDistro {
    use std::sync::OnceLock;
    static CURRENT: OnceLock<LinuxDistro> = OnceLock::new();
    *CURRENT.get_or_init(|| {
        let flatpak = std::path::Path::new("/.flatpak-info").exists() || std::env::var_os("FLATPAK_ID").is_some();
        let text = std::fs::read_to_string("/etc/os-release")
            .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
            .unwrap_or_default();
        detect(&text, flatpak)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ubuntu_and_derivatives_are_debian() {
        let ubuntu = "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\nVERSION_ID=\"24.04\"\n";
        assert_eq!(detect(ubuntu, false), LinuxDistro::Debian);
        let mint = "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n";
        assert_eq!(detect(mint, false), LinuxDistro::Debian);
        // Неизвестный производный: решает ID_LIKE.
        assert_eq!(detect("ID=zorin\nID_LIKE=\"ubuntu debian\"\n", false), LinuxDistro::Debian);
    }

    #[test]
    fn rpm_arch_and_suse_families() {
        assert_eq!(detect("ID=fedora\n", false), LinuxDistro::Fedora);
        assert_eq!(detect("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n", false), LinuxDistro::Fedora);
        assert_eq!(detect("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n", false), LinuxDistro::Suse);
        assert_eq!(detect("ID=manjaro\nID_LIKE=arch\n", false), LinuxDistro::Arch);
    }

    #[test]
    fn id_wins_over_id_like_and_unknown_is_other() {
        // ID идёт первым, даже если в файле он записан после ID_LIKE.
        assert_eq!(detect("ID_LIKE=debian\nID=fedora\n", false), LinuxDistro::Fedora);
        assert_eq!(detect("ID=nixos\n", false), LinuxDistro::Other);
        assert_eq!(detect("", false), LinuxDistro::Other);
        assert_eq!(detect("# комментарий\nбез равно\n", false), LinuxDistro::Other);
    }

    #[test]
    fn flatpak_overrides_the_host_distribution() {
        assert_eq!(detect("ID=ubuntu\n", true), LinuxDistro::Flatpak);
    }

    #[test]
    fn every_distro_has_a_command() {
        for d in [
            LinuxDistro::Debian,
            LinuxDistro::Fedora,
            LinuxDistro::Suse,
            LinuxDistro::Arch,
            LinuxDistro::Flatpak,
            LinuxDistro::Other,
        ] {
            assert!(!d.install_command().is_empty());
        }
        assert!(LinuxDistro::Debian.install_command().starts_with("sudo apt install"));
        assert!(LinuxDistro::Fedora.repository_note().contains("RPM Fusion"));
        assert!(LinuxDistro::Suse.repository_note().contains("Packman"));
        assert!(LinuxDistro::Flatpak.install_command().contains("ffmpeg-full"));
    }
}
