// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

/// Automatically deletes the path on [Drop].
pub struct PathGuard(pub std::path::PathBuf);

impl std::ops::Deref for PathGuard {
    type Target = std::path::PathBuf;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for PathGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for PathGuard {
    fn drop(&mut self) {
        if self.0.exists() {
            if self.0.is_dir() {
                std::fs::remove_dir_all(&self.0).unwrap();
            } else {
                std::fs::remove_file(&self.0).unwrap();
            }
        }
    }
}
