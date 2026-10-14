//! Windows integration: files, shortcuts, registry, PATH.
//! Everything is per-user (HKCU / %LOCALAPPDATA%), so no administrator rights are needed.

use std::ffi::c_void;
use std::io;
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::RegKey;

pub const APP_NAME: &str = "Wedit";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const EXE_NAME: &str = "wedit.exe";
pub const UNINSTALLER_NAME: &str = "uninstall.exe";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Wedit";
const PROG_ID: &str = "Wedit.SourceFile";
const EXTENSIONS: &[&str] = &[".rs", ".cs"];

pub static PAYLOAD: &[u8] = include_bytes!(env!("WEDIT_PAYLOAD_PATH"));

#[derive(Clone)]
pub struct Options {
    pub install_dir: PathBuf,
    pub start_menu: bool,
    pub desktop: bool,
    pub context_menu: bool,
    pub associate: bool,
    pub add_to_path: bool,
}

pub fn default_install_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\Public")).join("Programs").join(APP_NAME)
}

/// Install location of an existing installation, if any.
pub fn existing_install() -> Option<(PathBuf, String)> {
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(UNINSTALL_KEY).ok()?;
    let loc: String = key.get_value("InstallLocation").ok()?;
    let ver: String = key.get_value("DisplayVersion").unwrap_or_default();
    Some((PathBuf::from(loc), ver))
}

fn start_menu_lnk() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(r"Microsoft\Windows\Start Menu\Programs").join(format!("{APP_NAME}.lnk")))
}

fn desktop_lnk() -> Option<PathBuf> {
    dirs::desktop_dir().map(|d| d.join(format!("{APP_NAME}.lnk")))
}

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

pub type Step = (&'static str, fn(&Options) -> io::Result<()>);

/// Ordered install steps, so the UI can show progress.
pub fn install_steps() -> Vec<Step> {
    vec![
        ("Copying Wedit", step_copy_files),
        ("Creating shortcuts", step_shortcuts),
        ("Registering file types", step_shell_integration),
        ("Updating PATH", step_path),
        ("Registering uninstaller", step_register),
    ]
}

fn step_copy_files(o: &Options) -> io::Result<()> {
    std::fs::create_dir_all(&o.install_dir)?;
    let exe = o.install_dir.join(EXE_NAME);
    std::fs::write(&exe, PAYLOAD).map_err(|e| {
        if e.kind() == io::ErrorKind::PermissionDenied {
            err(format!("Cannot write {}.\nIf Wedit is running, close it and try again.", exe.display()))
        } else {
            e
        }
    })?;
    // The uninstaller is a copy of this setup program.
    let me = std::env::current_exe()?;
    let uninst = o.install_dir.join(UNINSTALLER_NAME);
    if me != uninst {
        std::fs::copy(&me, &uninst)?;
    }
    Ok(())
}

fn make_lnk(target: &Path, lnk: &Path) -> io::Result<()> {
    if let Some(parent) = lnk.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut sl = mslnk::ShellLink::new(target).map_err(|e| err(format!("shortcut: {e:?}")))?;
    sl.set_name(Some("Wedit code editor".into()));
    sl.set_working_dir(target.parent().map(|p| p.display().to_string()));
    sl.set_icon_location(Some(target.display().to_string()));
    sl.create_lnk(lnk).map_err(|e| err(format!("shortcut: {e:?}")))
}

fn step_shortcuts(o: &Options) -> io::Result<()> {
    let exe = o.install_dir.join(EXE_NAME);
    for (wanted, lnk) in [(o.start_menu, start_menu_lnk()), (o.desktop, desktop_lnk())] {
        let Some(lnk) = lnk else { continue };
        if wanted {
            make_lnk(&exe, &lnk)?;
        } else {
            let _ = std::fs::remove_file(&lnk);
        }
    }
    Ok(())
}

fn classes() -> io::Result<RegKey> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(r"Software\Classes", KEY_ALL_ACCESS)
}

fn set_command(parent: &RegKey, path: &str, label: &str, icon: &str, command: &str) -> io::Result<()> {
    let (key, _) = parent.create_subkey(path)?;
    key.set_value("", &label)?;
    key.set_value("Icon", &icon)?;
    let (cmd, _) = key.create_subkey("command")?;
    cmd.set_value("", &command)?;
    Ok(())
}

fn step_shell_integration(o: &Options) -> io::Result<()> {
    remove_shell_integration();
    let exe = o.install_dir.join(EXE_NAME).display().to_string();
    let icon = format!("\"{exe}\",0");
    let cls = classes()?;

    // Always register the application so "Open with" lists it nicely.
    let (app, _) = cls.create_subkey(format!(r"Applications\{EXE_NAME}"))?;
    app.set_value("FriendlyAppName", &APP_NAME)?;
    set_command(&app, r"shell\open", "Open", &icon, &format!("\"{exe}\" \"%1\""))?;
    let (types, _) = app.create_subkey("SupportedTypes")?;
    for ext in EXTENSIONS {
        types.set_value(ext, &"")?;
    }

    if o.associate {
        let (prog, _) = cls.create_subkey(PROG_ID)?;
        prog.set_value("", &"Source File (Wedit)")?;
        prog.create_subkey("DefaultIcon")?.0.set_value("", &icon)?;
        set_command(&prog, r"shell\open", "Open with Wedit", &icon, &format!("\"{exe}\" \"%1\""))?;
        for ext in EXTENSIONS {
            let (k, _) = cls.create_subkey(format!(r"{ext}\OpenWithProgids"))?;
            k.set_value(PROG_ID, &"")?;
        }
    }

    if o.context_menu {
        set_command(&cls, r"*\shell\Wedit", "Edit with Wedit", &icon, &format!("\"{exe}\" \"%1\""))?;
        set_command(&cls, r"Directory\shell\Wedit", "Open folder in Wedit", &icon, &format!("\"{exe}\" \"%V\""))?;
        set_command(&cls, r"Directory\Background\shell\Wedit", "Open folder in Wedit", &icon, &format!("\"{exe}\" \"%V\""))?;
    }
    notify_assoc_changed();
    Ok(())
}

fn remove_shell_integration() {
    let Ok(cls) = classes() else { return };
    for k in [
        format!(r"Applications\{EXE_NAME}"),
        PROG_ID.to_string(),
        r"*\shell\Wedit".into(),
        r"Directory\shell\Wedit".into(),
        r"Directory\Background\shell\Wedit".into(),
    ] {
        let _ = cls.delete_subkey_all(&k);
    }
    for ext in EXTENSIONS {
        if let Ok(k) = cls.open_subkey_with_flags(format!(r"{ext}\OpenWithProgids"), KEY_ALL_ACCESS) {
            let _ = k.delete_value(PROG_ID);
        }
    }
}

fn user_path() -> io::Result<(RegKey, String)> {
    let (env, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey("Environment")?;
    let path: String = env.get_value("Path").unwrap_or_default();
    Ok((env, path))
}

fn set_user_path(env: &RegKey, value: &str) -> io::Result<()> {
    // Preserve REG_EXPAND_SZ so %VARS% in the user's PATH keep working.
    let raw = winreg::RegValue {
        bytes: value.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect(),
        vtype: REG_EXPAND_SZ,
    };
    env.set_raw_value("Path", &raw)?;
    broadcast_env_change();
    Ok(())
}

fn path_without(path: &str, dir: &Path) -> Vec<String> {
    let dir = dir.display().to_string();
    path.split(';')
        .filter(|p| !p.is_empty() && !p.trim_end_matches('\\').eq_ignore_ascii_case(dir.trim_end_matches('\\')))
        .map(String::from)
        .collect()
}

fn step_path(o: &Options) -> io::Result<()> {
    let (env, path) = user_path()?;
    let mut parts = path_without(&path, &o.install_dir);
    if o.add_to_path {
        parts.push(o.install_dir.display().to_string());
    }
    let new = parts.join(";");
    if new != path {
        set_user_path(&env, &new)?;
    }
    Ok(())
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

fn step_register(o: &Options) -> io::Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL_KEY)?;
    let exe = o.install_dir.join(EXE_NAME).display().to_string();
    let uninst = o.install_dir.join(UNINSTALLER_NAME).display().to_string();
    key.set_value("DisplayName", &APP_NAME)?;
    key.set_value("DisplayVersion", &VERSION)?;
    key.set_value("Publisher", &APP_NAME)?;
    key.set_value("DisplayIcon", &format!("\"{exe}\",0"))?;
    key.set_value("InstallLocation", &o.install_dir.display().to_string())?;
    key.set_value("UninstallString", &format!("\"{uninst}\" --uninstall"))?;
    key.set_value("QuietUninstallString", &format!("\"{uninst}\" --uninstall --quiet"))?;
    key.set_value("EstimatedSize", &((dir_size(&o.install_dir) / 1024) as u32))?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    // Remember the chosen options so the uninstaller can undo PATH changes.
    key.set_value("WeditAddedToPath", &(o.add_to_path as u32))?;
    Ok(())
}

/// Remove everything. The install folder itself is deleted by a helper process
/// once this (possibly running-from-that-folder) uninstaller exits.
pub fn uninstall(install_dir: &Path) -> io::Result<()> {
    if let Some(l) = start_menu_lnk() {
        let _ = std::fs::remove_file(l);
    }
    if let Some(l) = desktop_lnk() {
        let _ = std::fs::remove_file(l);
    }
    remove_shell_integration();
    notify_assoc_changed();
    if let Ok((env, path)) = user_path() {
        let parts = path_without(&path, install_dir);
        let new = parts.join(";");
        if new != path {
            let _ = set_user_path(&env, &new);
        }
    }
    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY);

    let exe = install_dir.join(EXE_NAME);
    if exe.exists() {
        std::fs::remove_file(&exe).map_err(|_| err("Wedit appears to be running. Close it and try again."))?;
    }
    Ok(())
}

/// Delete the install folder after this process exits.
pub fn schedule_folder_removal(install_dir: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Only ever remove a folder that looks like a Wedit install.
    if !install_dir.join(UNINSTALLER_NAME).exists() && !install_dir.join(EXE_NAME).exists() {
        return;
    }
    let cmd = format!("ping 127.0.0.1 -n 3 > nul & rmdir /s /q \"{}\"", install_dir.display());
    let _ = std::process::Command::new("cmd").raw_arg("/C").raw_arg(&cmd).creation_flags(CREATE_NO_WINDOW).spawn();
}

pub fn launch(install_dir: &Path) {
    let _ = std::process::Command::new(install_dir.join(EXE_NAME)).current_dir(install_dir).spawn();
}

// ----------------------------------------------------------------- Win32 notifications

#[link(name = "shell32")]
extern "system" {
    fn SHChangeNotify(event_id: i32, flags: u32, item1: *const c_void, item2: *const c_void);
}

#[link(name = "user32")]
extern "system" {
    fn SendMessageTimeoutW(hwnd: isize, msg: u32, wparam: usize, lparam: isize, flags: u32, timeout: u32, result: *mut usize) -> isize;
}

fn notify_assoc_changed() {
    const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, 0, std::ptr::null(), std::ptr::null()) };
}

fn broadcast_env_change() {
    const HWND_BROADCAST: isize = 0xffff;
    const WM_SETTINGCHANGE: u32 = 0x001A;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    let env: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, env.as_ptr() as isize, SMTO_ABORTIFHUNG, 2000, &mut result);
    }
}
