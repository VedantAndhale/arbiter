//! Native credential storage. Never falls back to a plaintext file.
use std::io;

pub trait CredentialStore: Send + Sync {
    fn persistent(&self) -> bool;
    fn read(&self, target: &str) -> io::Result<Option<String>>;
    fn write(&self, target: &str, secret: &str) -> io::Result<()>;
    fn delete(&self, target: &str) -> io::Result<()>;
}
pub struct NativeCredentials;

#[cfg(windows)]
impl CredentialStore for NativeCredentials {
    fn persistent(&self) -> bool {
        true
    }
    fn read(&self, target: &str) -> io::Result<Option<String>> {
        use windows_sys::Win32::{
            Foundation::{ERROR_NOT_FOUND, GetLastError},
            Security::Credentials::*,
        };
        let name = wide(target)?;
        let mut pointer = std::ptr::null_mut();
        // Windows owns the returned allocation until CredFree. Copy only the blob.
        unsafe {
            if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) == 0 {
                let code = GetLastError();
                return if code == ERROR_NOT_FOUND { Ok(None) } else { Err(io::Error::from_raw_os_error(code as i32)) };
            }
            let credential = &*pointer;
            let bytes = if credential.CredentialBlobSize == 0 {
                vec![]
            } else {
                std::slice::from_raw_parts(credential.CredentialBlob, credential.CredentialBlobSize as usize).to_vec()
            };
            CredFree(pointer.cast());
            String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid stored credential encoding"))
        }
    }
    fn write(&self, target: &str, secret: &str) -> io::Result<()> {
        use windows_sys::Win32::Security::Credentials::*;
        if secret.is_empty() || secret.len() > 2500 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid credential size"));
        }
        let mut name = wide(target)?;
        let mut user = wide("Arbiter")?;
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_mut_ptr(),
            CredentialBlobSize: secret.len() as u32,
            CredentialBlob: secret.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: user.as_mut_ptr(),
            ..Default::default()
        };
        // All pointers stay valid for the synchronous call; Windows copies bytes.
        if unsafe { CredWriteW(&credential, 0) } == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }
    fn delete(&self, target: &str) -> io::Result<()> {
        use windows_sys::Win32::{
            Foundation::{ERROR_NOT_FOUND, GetLastError},
            Security::Credentials::*,
        };
        let name = wide(target)?;
        unsafe {
            if CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) != 0 {
                return Ok(());
            }
            let code = GetLastError();
            if code == ERROR_NOT_FOUND { Ok(()) } else { Err(io::Error::from_raw_os_error(code as i32)) }
        }
    }
}
#[cfg(windows)]
fn wide(value: &str) -> io::Result<Vec<u16>> {
    if value.contains('\0') || value.len() > 500 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid credential target"));
    }
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}
#[cfg(not(windows))]
impl CredentialStore for NativeCredentials {
    fn persistent(&self) -> bool {
        false
    }
    fn read(&self, _: &str) -> io::Result<Option<String>> {
        Ok(None)
    }
    fn write(&self, _: &str, _: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Native credential storage unavailable; use session-only storage",
        ))
    }
    fn delete(&self, _: &str) -> io::Result<()> {
        Ok(())
    }
}

/// Test backend: no OS credentials or files touched.
#[derive(Default)]
pub struct MemoryCredentials(std::sync::Mutex<std::collections::HashMap<String, String>>);
impl CredentialStore for MemoryCredentials {
    fn persistent(&self) -> bool {
        true
    }
    fn read(&self, target: &str) -> io::Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(target).cloned())
    }
    fn write(&self, target: &str, secret: &str) -> io::Result<()> {
        self.0.lock().unwrap().insert(target.into(), secret.into());
        Ok(())
    }
    fn delete(&self, target: &str) -> io::Result<()> {
        self.0.lock().unwrap().remove(target);
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "explicit native verification creates and removes a unique dummy OS credential"]
    fn native_dummy_credential_roundtrip() {
        let target = format!(
            "Arbiter/Test/{}/{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        );
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = NativeCredentials.delete(&self.0);
            }
        }
        let _cleanup = Cleanup(target.clone());
        assert!(NativeCredentials.read(&target).unwrap().is_none());
        NativeCredentials.write(&target, "dummy-not-a-provider-key").unwrap();
        assert_eq!(NativeCredentials.read(&target).unwrap().as_deref(), Some("dummy-not-a-provider-key"));
        NativeCredentials.delete(&target).unwrap();
        assert!(NativeCredentials.read(&target).unwrap().is_none());
    }
}
