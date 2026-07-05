use crate::CliArgs;

pub fn parse_cli_args() -> Result<CliArgs, Box<dyn std::error::Error>> {
    parse_cli_args_from(std::env::args().skip(1))
}

fn parse_cli_args_from(
    args: impl IntoIterator<Item = String>,
) -> Result<CliArgs, Box<dyn std::error::Error>> {
    // What：解析调用方传入的 CLI 参数序列。
    // Why：入口只支持 profile + 单个命令，先用最小解析避免把 CLI 合同扩成第二套配置系统。
    let mut command = None;
    let mut profile = None;
    let mut create_profile = None;
    let mut args_json = None;
    let mut auth_token = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "server" | "init" => {
                if command.is_some() {
                    return Err(format!("顶层命令不能重复: {arg}").into());
                }
                command = Some(arg);
            }
            "--profile" => profile = Some(args.next().ok_or("--profile 缺少 profile 名称")?),
            "--create_profile" => {
                if create_profile.is_some() {
                    return Err("--create_profile 不能重复使用".into());
                }
                let name = args.next().ok_or("--create_profile 缺少 profile 名称")?;
                validate_profile_name(name.as_str())?;
                create_profile = Some(name);
            }
            "--args" => {
                if args_json.is_some() {
                    return Err("--args 不能重复使用".into());
                }
                args_json = Some(args.next().ok_or("--args 缺少 JSON object")?);
            }
            "--auth" => auth_token = Some(args.next().ok_or("--auth 缺少 auth_token")?),
            _ => return Err(format!("未知参数: {arg}").into()),
        }
    }

    if let Some(command) = command.as_deref().filter(|_| args_json.is_some()) {
        return Err(format!("{command} 不支持 --args").into());
    }
    if create_profile.is_some() {
        if profile.is_some() {
            return Err("--create_profile 不支持 --profile".into());
        }
        if let Some(command) = command.as_deref() {
            return Err(format!("--create_profile 不能和 {command} 同时使用").into());
        }
        if args_json.is_some() {
            return Err("--create_profile 不支持 --args".into());
        }
        if auth_token.is_some() {
            return Err("--create_profile 不支持 --auth".into());
        }
    }
    if auth_token.is_some() {
        if let Some(command) = command.as_deref() {
            return Err(format!("--auth 不能和 {command} 同时使用").into());
        }
        if args_json.is_some() {
            return Err("--auth 不支持 --args".into());
        }
    }

    Ok(CliArgs {
        command,
        profile,
        create_profile,
        args_json,
        auth_token,
    })
}

fn validate_profile_name(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let valid = name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if !valid {
        return Err("profile 名称必须匹配 [a-z][a-z0-9_]*".into());
    }
    if matches!(name, "postgres" | "template0" | "template1") {
        return Err("profile 名称是保留名".into());
    }
    Ok(())
}

pub fn parse_args_json(args_json: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Why：先把 CLI 输入收敛成一个 JSON object，后面的工具分发才不会接收多种入口形状。
    let request_args = serde_json::from_str::<serde_json::Value>(args_json)?;

    if !request_args.is_object() {
        return Err("--args 必须是 JSON object".into());
    }

    Ok(request_args)
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args_from;

    #[test]
    fn parse_cli_args_accepts_create_profile_option() {
        let args = vec!["--create_profile".to_string(), "rikocodex".to_string()];

        let cli_args = parse_cli_args_from(args).unwrap();

        assert_eq!(cli_args.create_profile.as_deref(), Some("rikocodex"));
        assert!(cli_args.command.is_none());
    }

    #[test]
    fn parse_cli_args_accepts_share_create_profile() {
        let args = vec!["--create_profile".to_string(), "share".to_string()];

        let cli_args = parse_cli_args_from(args).unwrap();

        assert_eq!(cli_args.create_profile.as_deref(), Some("share"));
    }

    #[test]
    fn parse_cli_args_rejects_resetdb_command() {
        let error = match parse_cli_args_from(vec![
            "--profile".to_string(),
            "maccodex".to_string(),
            "resetdb".to_string(),
        ]) {
            Ok(_) => panic!("resetdb should not be a top-level command"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "未知参数: resetdb");
    }

    #[test]
    fn parse_cli_args_rejects_invalid_create_profile_names() {
        for name in [
            "Riko",
            "riKo",
            "1riko",
            "riko-codex",
            "postgres",
            "template0",
        ] {
            let error =
                match parse_cli_args_from(vec!["--create_profile".to_string(), name.to_string()]) {
                    Ok(_) => panic!("invalid profile name should be rejected: {name}"),
                    Err(error) => error,
                };

            assert!(error.to_string().starts_with("profile 名称"));
        }
    }

    #[test]
    fn parse_cli_args_rejects_create_profile_with_other_entrypoints() {
        for extra in ["server", "init", "--profile", "--args", "--auth"] {
            let mut args = vec![
                "--create_profile".to_string(),
                "rikocodex".to_string(),
                extra.to_string(),
            ];
            if matches!(extra, "--profile" | "--args" | "--auth") {
                args.push("value".to_string());
            }

            let error = match parse_cli_args_from(args) {
                Ok(_) => panic!("--create_profile should reject {extra}"),
                Err(error) => error,
            };

            assert!(error.to_string().contains("--create_profile"));
        }
    }

    #[test]
    fn parse_cli_args_rejects_missing_option_values() {
        for (option, expected) in [
            ("--profile", "--profile 缺少 profile 名称"),
            ("--args", "--args 缺少 JSON object"),
            ("--auth", "--auth 缺少 auth_token"),
        ] {
            let error = match parse_cli_args_from(vec![option.to_string()]) {
                Ok(_) => panic!("{option} should reject missing value"),
                Err(error) => error,
            };

            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn parse_cli_args_rejects_unknown_command() {
        let error = match parse_cli_args_from(vec!["migrate".to_string()]) {
            Ok(_) => panic!("unknown command should be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "未知参数: migrate");
    }

    #[test]
    fn parse_cli_args_rejects_duplicate_command() {
        let error = match parse_cli_args_from(vec!["init".to_string(), "server".to_string()]) {
            Ok(_) => panic!("duplicate command should be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "顶层命令不能重复: server");
    }

    #[test]
    fn parse_cli_args_rejects_args_for_init_command() {
        let args = vec![
            "--profile".to_string(),
            "maccodex".to_string(),
            "init".to_string(),
            "--args".to_string(),
            "{}".to_string(),
        ];
        let error = match parse_cli_args_from(args) {
            Ok(_) => panic!("init should reject --args"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "init 不支持 --args");
    }

    #[test]
    fn parse_cli_args_rejects_args_for_server_command() {
        let args = vec!["server".to_string(), "--args".to_string(), "{}".to_string()];
        let error = match parse_cli_args_from(args) {
            Ok(_) => panic!("server should reject --args"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "server 不支持 --args");
    }

    #[test]
    fn parse_cli_args_rejects_auth_with_other_entrypoints() {
        let cases = [
            vec![
                "--profile".to_string(),
                "maccodex".to_string(),
                "--auth".to_string(),
                "token".to_string(),
                "--args".to_string(),
                "{}".to_string(),
            ],
            vec![
                "--profile".to_string(),
                "maccodex".to_string(),
                "init".to_string(),
                "--auth".to_string(),
                "token".to_string(),
            ],
            vec![
                "server".to_string(),
                "--auth".to_string(),
                "token".to_string(),
            ],
        ];

        for args in cases {
            let error = match parse_cli_args_from(args) {
                Ok(_) => panic!("--auth should reject mixed entrypoints"),
                Err(error) => error,
            };

            assert!(error.to_string().contains("--auth"));
        }
    }
}
