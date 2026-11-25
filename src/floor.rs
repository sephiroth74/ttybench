//! The floor of a terminal's startup: `ttybench-floor`, the least that an
//! app of AppKit does to show a window, launched as the terminal measured
//! launches, by `open` of its bundle or of its executable, and measured as
//! the terminal is, to its window as the window server lists it. A
//! terminal's startup less the floor's of the same round is its own part,
//! which the Mac's state moves less than the startup itself, as two days'
//! sessions tell.

use std::io;
use std::path::{Path, PathBuf};

use crate::config::{Launch, Terminal};
use crate::tools;

/// The floor's bundle's name.
pub const BUNDLE: &str = "ttybench-floor.app";

/// The floor's executable's name, in its bundle as beside the tools.
pub const EXECUTABLE: &str = "ttybench-floor";

/// The floor's bundle identifier.
pub const IDENTIFIER: &str = "io.github.sephiroth74.ttybench.floor";

/// The floor's executable: `ttybench-floor` beside the tool or on the
/// `PATH`, as the dispatcher finds the tools.
pub fn binary() -> Result<PathBuf, String> {
    let dirs = tools::search_dirs(
        std::env::current_exe().ok().as_deref(),
        std::env::var_os("PATH").as_deref(),
    );
    tools::find("floor", &dirs).ok_or_else(|| {
        "there is no ttybench-floor beside the tool or on the PATH, which the floor launches"
            .to_owned()
    })
}

/// Makes the floor's bundle in `folder` of the executable at `executable`,
/// anew where it holds another executable: its `Info.plist` and its
/// executable copied; the bundle's path.
pub fn bundle(folder: &Path, executable: &Path) -> io::Result<PathBuf> {
    let bundle = folder.join(BUNDLE);
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    let copy = macos.join(EXECUTABLE);
    let current = std::fs::read(executable)?;
    if std::fs::read(&copy).is_ok_and(|kept| kept == current) {
        return Ok(bundle);
    }
    std::fs::create_dir_all(&macos)?;
    std::fs::write(contents.join("Info.plist"), info_plist())?;
    let temporary = macos.join(format!(".{EXECUTABLE}.new"));
    std::fs::write(&temporary, &current)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&temporary, &copy)?;
    Ok(bundle)
}

/// The floor's bundle of the tool's cache, made anew of `executable` as
/// it changes: `~/Library/Caches/ttybench`.
pub fn cached_bundle(executable: &Path) -> io::Result<PathBuf> {
    let home = std::env::home_dir().ok_or_else(|| io::Error::other("no home folder"))?;
    bundle(&home.join("Library/Caches/ttybench"), executable)
}

/// The floor of `measured`, of the floor's bundle `bundle`: launched as
/// the terminal is, by `open` or by its executable, of no environment of
/// the terminal's.
pub fn terminal_of(measured: &Terminal, bundle: &Path) -> Terminal {
    let launch = match measured.launch {
        Launch::Open => Launch::Open,
        Launch::Argv(_) => Launch::Argv(vec![format!("{{app}}/Contents/MacOS/{EXECUTABLE}")]),
    };
    Terminal {
        app: bundle.to_path_buf(),
        launch,
        env: Default::default(),
        beside: None,
        features: Default::default(),
        quit: Default::default(),
    }
}

/// The floor's `Info.plist`.
fn info_plist() -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
            "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\">\n<dict>\n",
            "\t<key>CFBundleExecutable</key>\n\t<string>{executable}</string>\n",
            "\t<key>CFBundleIdentifier</key>\n\t<string>{identifier}</string>\n",
            "\t<key>CFBundleName</key>\n\t<string>ttybench floor</string>\n",
            "\t<key>CFBundlePackageType</key>\n\t<string>APPL</string>\n",
            "\t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n",
            "\t<key>CFBundleVersion</key>\n\t<string>{version}</string>\n",
            "\t<key>LSMinimumSystemVersion</key>\n\t<string>26.0</string>\n",
            "\t<key>NSHighResolutionCapable</key>\n\t<true/>\n",
            "\t<key>NSPrincipalClass</key>\n\t<string>NSApplication</string>\n",
            "</dict>\n</plist>\n"
        ),
        executable = EXECUTABLE,
        identifier = IDENTIFIER,
        version = env!("CARGO_PKG_VERSION"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_floor_s_bundle_holds_its_plist_and_its_executable_made_anew_as_it_changes() {
        let folder =
            std::env::temp_dir().join(format!("ttybench-floor-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let executable = folder.join("bin");
        std::fs::write(&executable, b"one").unwrap();
        let made = bundle(&folder, &executable).unwrap();
        assert_eq!(made, folder.join(BUNDLE));
        let plist = std::fs::read_to_string(made.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains(IDENTIFIER) && plist.contains("<string>ttybench-floor</string>"));
        let copy = made.join("Contents/MacOS").join(EXECUTABLE);
        assert_eq!(std::fs::read(&copy).unwrap(), b"one");
        // Another executable, the bundle's copy made anew.
        std::fs::write(&executable, b"two").unwrap();
        bundle(&folder, &executable).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"two");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_floor_launches_as_the_terminal_measured() {
        let bundle = Path::new("/tmp/ttybench-floor.app");
        let opened = Terminal {
            app: PathBuf::from("/Applications/Lokyr.app"),
            launch: Launch::Open,
            env: [("XDG_CONFIG_HOME".to_owned(), "/tmp/x".to_owned())].into(),
            beside: None,
            features: Default::default(),
            quit: Default::default(),
        };
        let floor = terminal_of(&opened, bundle);
        assert_eq!(floor.launch, Launch::Open);
        assert!(floor.env.is_empty());
        let direct = Terminal {
            launch: Launch::Argv(vec![
                "{app}/Contents/MacOS/kitty".to_owned(),
                "{command}".to_owned(),
            ]),
            ..opened
        };
        let floor = terminal_of(&direct, bundle);
        assert_eq!(
            floor.argv(Path::new("/tmp/c"), Path::new("/tmp")),
            Some(vec![
                "/tmp/ttybench-floor.app/Contents/MacOS/ttybench-floor".to_owned()
            ])
        );
    }
}
