//! Path expansion for user-configured paths.
//!
//! Expands a leading `~` (or `~/…`) to `$HOME`, and `$VAR` / `${VAR}` anywhere
//! in the string to the environment-variable value. Unknown variables are left
//! intact so the resulting path surfaces obviously in error messages.

use std::path::{Path, PathBuf};

pub fn expand_path(p: &Path) -> PathBuf {
    match p.to_str() {
        Some(s) => PathBuf::from(expand(s)),
        None => p.to_path_buf(),
    }
}

pub fn expand(s: &str) -> String {
    let with_home = expand_tilde(s);
    expand_env(&with_home)
}

fn expand_tilde(s: &str) -> String {
    if s == "~" {
        return home().unwrap_or_else(|| "~".into());
    }
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = home()
    {
        return format!("{home}/{rest}");
    }
    s.to_string()
}

fn home() -> Option<String> {
    std::env::var("HOME").ok().filter(|s| !s.is_empty())
}

fn expand_env(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'{' {
                if let Some(end) = s[i + 2..].find('}') {
                    let name = &s[i + 2..i + 2 + end];
                    match std::env::var(name) {
                        Ok(v) => out.push_str(&v),
                        Err(_) => out.push_str(&s[i..i + 2 + end + 1]),
                    }
                    i += 2 + end + 1;
                    continue;
                }
            } else if is_name_start(bytes[i + 1]) {
                let mut j = i + 1;
                while j < bytes.len() && is_name_cont(bytes[j]) {
                    j += 1;
                }
                let name = &s[i + 1..j];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => out.push_str(&s[i..j]),
                }
                i = j;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}
fn is_name_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }
    impl EnvGuard {
        fn set(key: &'static str, val: &str) -> Self {
            let prev = std::env::var(key).ok();
            // SAFETY: tests in this module are annotated as serial via a mutex.
            unsafe { std::env::set_var(key, val) };
            Self { key, prev }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prev {
                    Some(v) => std::env::set_var(self.key, v),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    // Serialize env-mutating tests.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn expands_leading_tilde() {
        let _g = ENV_LOCK.lock().unwrap();
        let _h = EnvGuard::set("HOME", "/home/testuser");
        assert_eq!(expand("~"), "/home/testuser");
        assert_eq!(expand("~/personal/roms"), "/home/testuser/personal/roms");
    }

    #[test]
    fn tilde_not_at_start_is_left_alone() {
        let _g = ENV_LOCK.lock().unwrap();
        let _h = EnvGuard::set("HOME", "/home/testuser");
        assert_eq!(expand("/opt/~/foo"), "/opt/~/foo");
        assert_eq!(expand("foo~bar"), "foo~bar");
    }

    #[test]
    fn expands_dollar_var_and_braced() {
        let _g = ENV_LOCK.lock().unwrap();
        let _u = EnvGuard::set("SHIPYARD_TEST_USER", "jules");
        assert_eq!(
            expand("/home/$SHIPYARD_TEST_USER/x"),
            "/home/jules/x"
        );
        assert_eq!(
            expand("/home/${SHIPYARD_TEST_USER}_bkp"),
            "/home/jules_bkp"
        );
    }

    #[test]
    fn unknown_var_is_preserved_literally() {
        let _g = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("SHIPYARD_NO_SUCH_VAR") };
        assert_eq!(
            expand("/x/$SHIPYARD_NO_SUCH_VAR/y"),
            "/x/$SHIPYARD_NO_SUCH_VAR/y"
        );
        assert_eq!(
            expand("/x/${SHIPYARD_NO_SUCH_VAR}/y"),
            "/x/${SHIPYARD_NO_SUCH_VAR}/y"
        );
    }

    #[test]
    fn combined_tilde_and_env() {
        let _g = ENV_LOCK.lock().unwrap();
        let _h = EnvGuard::set("HOME", "/home/testuser");
        let _u = EnvGuard::set("SHIPYARD_SUB", "saves");
        assert_eq!(expand("~/$SHIPYARD_SUB/a"), "/home/testuser/saves/a");
    }

    #[test]
    fn expand_path_roundtrip() {
        let _g = ENV_LOCK.lock().unwrap();
        let _h = EnvGuard::set("HOME", "/home/testuser");
        let p = Path::new("~/roms/oot.z64");
        assert_eq!(expand_path(p), PathBuf::from("/home/testuser/roms/oot.z64"));
    }
}
