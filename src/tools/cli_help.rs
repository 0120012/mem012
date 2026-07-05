pub(crate) fn print_agent_help(
    config: &crate::config::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：输出给 Agent 读取的 CLI 能力入口元数据。
    // Why：category 白名单来自运行时配置，避免 help 和实际写入校验出现两套来源。
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "state": "success",
            "tool": "help",
            "data": {
                "skill": {
                    "name": "mem012-memory-skill"
                },
                "categories": {
                    "cateory_list": config.category_index_list()
                },
                "failure_instruction": "任一 mem012 命令失败后，禁止猜测或重复尝试其他 mem012/file/strings/grep 探测命令；立即停止，并向用户报告失败命令、退出码和错误输出。"
            },
            "error": null
        }))?
    );
    Ok(())
}

pub(crate) fn agent_help_requested(args: &[String]) -> bool {
    // What：识别 Agent 常见的 help 误调用形态。
    // Why：help 必须在严格 CLI 解析前短路，避免 Agent 失败后继续枚举命令探测二进制。
    let mut skip_value = false;
    for (index, arg) in args.iter().enumerate() {
        if skip_value {
            skip_value = false;
            continue;
        }
        match arg.as_str() {
            "--help" | "help" | "--tool=help" => return true,
            "--tool" if args.get(index + 1).is_some_and(|value| value == "help") => return true,
            "--profile" | "--create_profile" | "--args" | "--auth" => skip_value = true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::agent_help_requested;

    #[test]
    fn agent_help_requested_ignores_create_profile_value() {
        assert!(!agent_help_requested(&[
            "--create_profile".to_string(),
            "help".to_string()
        ]));
        assert!(agent_help_requested(&["--help".to_string()]));
    }
}
