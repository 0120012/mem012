pub(crate) fn init_auth_file_path() -> Result<std::path::PathBuf, String> {
    // What：返回 init grant 在本机的固定授权文件路径。
    // Why：CLI 写入和 create_memory 消费必须使用同一位置，避免授权文件被写到一个路径、读取却去另一个路径。
    let home = std::env::var_os("HOME").ok_or("HOME 未设置，无法定位 init auth file")?;
    Ok(std::path::PathBuf::from(home)
        .join(".auth")
        .join("auth_file.mem"))
}

pub(crate) fn write_init_auth_file(
    path: &std::path::Path,
    grant: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let Some(parent) = path.parent() else {
        return Err("init auth file 路径缺少父目录".into());
    };
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;

    let mut bytes = serde_json::to_vec(grant)?;
    bytes.push(b'\n');
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::{init_auth_file_path, write_init_auth_file};

    #[test]
    fn init_auth_file_path_uses_fixed_auth_location() {
        let path = init_auth_file_path().unwrap();

        assert!(path.ends_with(".auth/auth_file.mem"));
    }

    #[test]
    fn write_init_auth_file_writes_grant_with_private_permissions() {
        let root =
            std::env::temp_dir().join(format!("mem012_auth_write_test_{}", std::process::id()));
        let path = root.join(".auth").join("auth_file.mem");
        let grant = serde_json::json!({
            "version": 1,
            "payload": {
                "grant_id": "grant",
                "scope": "init:create",
                "iat": 100,
                "exp": 400,
                "nonce": "nonce"
            },
            "signature": "signature"
        });

        write_init_auth_file(&path, &grant).unwrap();

        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&saved).unwrap(),
            grant
        );
        #[cfg(unix)]
        {
            assert_eq!(
                std::fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
