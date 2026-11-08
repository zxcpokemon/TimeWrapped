//! One shared policy for discovery, historical analytics and the application list.
//! A process is not an app merely because it runs in the user's session.
pub fn supporting_process(exe: &str) -> bool {
    let id=exe.to_ascii_lowercase();
    ["applicationframehost.exe","runtimebroker.exe","backgroundtaskhost.exe",
     "startmenuexperiencehost.exe","shellexperiencehost.exe","searchhost.exe",
     "searchapp.exe","textinputhost.exe","widgets.exe","widgetservice.exe",
     "lockapp.exe","logonui.exe","dwm.exe","shellhost.exe","openwith.exe",
     "nvidia overlay.exe","nvsphelper64.exe","nvsphelper.exe","socialclubhelper.exe","conhost.exe","openconsole.exe",
     "crashpad_handler.exe","crashreporter.exe","update.exe","updater.exe",
     "squirrel.exe","msedgewebview2.exe","steamwebhelper.exe","discordsystemhelper.exe",
     "filewatcher.exe","defendersessionhelper.exe","vctip.exe","node_repl.exe",
     "cargo.exe","rustc.exe","rustdoc.exe","codex.exe","ecef_process.exe",
     "eaclauncher.exe","easyanticheat.exe","easyanticheat_eos.exe",
     "cefsharp.browsersubprocess.exe","qtwebengineprocess.exe","dllhost.exe",
     "werfault.exe","wermgr.exe","audiodg.exe","svchost.exe"]
        .contains(&id.as_str())
        || id.starts_with("codex-") || id.starts_with("timewrapped-")
        || id.ends_with("-crash-handler.exe") || id.ends_with("_cef_helper.exe")
}

pub fn eligible(exe:&str, has_window:bool, known_app:bool, same_exe_child:bool)->bool {
    !supporting_process(exe) && (has_window || (known_app && !same_exe_child))
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn support_processes_never_become_apps_even_if_they_show_a_window() {
        for exe in ["ApplicationFrameHost.exe","cargo.exe","codex.exe","codex-code-mode-host.exe","codex-command-runner-0.166.0-alpha.51.exe","codex-computer-use.exe","DiscordSystemHelper.exe","EACLauncher.exe","ecef_process.exe","FileWatcher.exe"] {
            assert!(!eligible(exe,true,true,false),"{exe}");
        }
    }
    #[test] fn ordinary_apps_need_window_evidence_and_renderers_do_not_extend_lifetime() {
        for exe in ["ChatGPT.exe","Code.exe","Discord.exe","Spotify.exe","dota2.exe","unknown-app.exe"] {
            assert!(!eligible(exe,false,false,false));
            assert!(eligible(exe,true,false,false));
            assert!(eligible(exe,false,true,false));
            assert!(!eligible(exe,false,true,true));
            assert!(eligible(exe,true,true,true));
        }
    }
}
