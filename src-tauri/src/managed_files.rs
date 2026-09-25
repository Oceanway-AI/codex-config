//! Validate owned files before transactions; never follow links into another profile.
use std::{fs, io::Read, path::{Path, PathBuf}};
use sha2::{Digest, Sha256};

pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect a managed configuration file.".into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Refusing a linked or non-regular configuration file.".into());
    }
    let file = fs::File::open(path).map_err(|_| "Cannot open a managed configuration file.")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if file.metadata().map_err(|_| "Cannot inspect file links.")?.nlink() != 1 {
            return Err("Refusing a hard-linked configuration file.".into());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Refusing a reparse-point configuration file.".into());
        }
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0
            || information.nNumberOfLinks != 1
        {
            return Err("Cannot verify a single-link configuration file.".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)
        .map_err(|_| "Cannot read a managed configuration file.")?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Managed configuration exceeds the 16 MiB safety limit.".into());
    }
    Ok(Some(bytes))
}

pub fn text(path: &Path) -> Result<Option<String>, String> {
    read_optional(path)?.map(|bytes| String::from_utf8(bytes)
        .map_err(|_| "Managed configuration is not valid UTF-8.".into())).transpose()
}

pub fn file_hash(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|_| "Cannot read the bundled image MCP runtime.")?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|_| "Cannot hash the bundled image MCP runtime.")?;
        if count == 0 { break; }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub struct Snapshot(Vec<(PathBuf, Option<Vec<u8>>)>);

impl Snapshot {
    pub fn capture(home: &Path, names: &[&str]) -> Result<Self, String> {
        names.iter().map(|name| {
            let path = home.join(name);
            Ok((path.clone(), read_optional(&path)?))
        }).collect::<Result<Vec<_>, String>>().map(Self)
    }

    pub fn rollback(&self, cause: String) -> String {
        let mut failed = Vec::new();
        for (path, bytes) in &self.0 {
            let result = match bytes {
                Some(bytes) => super::write_private_atomic(path, bytes),
                None => fs::remove_file(path).or_else(|error|
                    if error.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(error) }),
            };
            if result.is_err() { failed.push(path.file_name().unwrap_or_default().to_string_lossy().into_owned()); }
        }
        if failed.is_empty() { format!("{cause} 本次修改已回滚。") }
        else { format!("{cause} 回滚失败，请保留备份：{}", failed.join(", ")) }
    }
}
