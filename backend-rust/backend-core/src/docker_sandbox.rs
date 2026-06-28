use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command as TokioCommand;

// ==================== Docker 沙箱执行器 ====================

/// Docker 沙箱配置
#[derive(Clone, Debug)]
pub struct DockerSandboxConfig {
    /// 是否启用 Docker 沙箱（false 则降级为直接执行）
    pub enabled: bool,
    /// Docker 镜像名称
    pub image: String,
    /// 容器内挂载的工作目录
    pub container_workspace: String,
    /// 执行超时（秒）
    pub timeout_secs: u64,
    /// 内存限制（如 "512m"）
    pub memory_limit: String,
    /// 是否允许网络访问
    pub network_enabled: bool,
    /// 只读 rootfs
    pub read_only: bool,
}

impl Default for DockerSandboxConfig {
    fn default() -> Self {
        DockerSandboxConfig {
            enabled: false,
            image: "ubuntu:22.04".to_string(),
            container_workspace: "/workspace".to_string(),
            timeout_secs: 60,
            memory_limit: "512m".to_string(),
            network_enabled: false,
            read_only: true,
        }
    }
}

/// 命令执行结果
#[derive(Debug)]
pub struct SandboxResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    /// 是否为沙箱模式执行
    pub sandboxed: bool,
}

// ==================== Docker 沙箱 ====================

/// Docker 沙箱执行器，支持在容器内执行命令，实现进程级隔离。
///
/// 降级策略：如果 Docker 不可用，自动降级为本地直接执行。
pub struct DockerSandbox {
    config: DockerSandboxConfig,
    host_workspace: PathBuf,
    docker_available: bool,
}

impl DockerSandbox {
    pub fn new(config: DockerSandboxConfig, host_workspace: PathBuf) -> Self {
        // 检测 Docker 是否可用
        let docker_available = config.enabled && Self::check_docker_available();

        DockerSandbox {
            config,
            host_workspace,
            docker_available,
        }
    }

    /// 检测 Docker 是否安装且可访问
    fn check_docker_available() -> bool {
        std::process::Command::new("docker")
            .arg("version")
            .arg("--format")
            .arg("{{.Server.Version}}")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// 执行命令（自动选择沙箱或本地模式）
    pub async fn execute(
        &self,
        command: &str,
        cwd: Option<&str>,
    ) -> Result<SandboxResult, String> {
        if self.docker_available {
            self.execute_in_container(command, cwd).await
        } else {
            self.execute_locally(command, cwd).await
        }
    }

    /// 在 Docker 容器内执行
    async fn execute_in_container(
        &self,
        command: &str,
        cwd: Option<&str>,
    ) -> Result<SandboxResult, String> {
        let host_workspace = self.host_workspace.to_string_lossy().to_string();
        let work_dir = cwd
            .map(|c| format!("{}{}", self.config.container_workspace, c))
            .unwrap_or_else(|| self.config.container_workspace.clone());

        // 将 host 目录路径转换为 Docker 绝对路径（Windows 路径处理）
        let host_workspace_docker = if cfg!(windows) {
            // Windows: C:\foo\bar → /c/foo/bar (Docker Desktop)
            let path = host_workspace.replace('\\', "/");
            if path.len() >= 2 && path.chars().nth(1) == Some(':') {
                format!("/{}{}", 
                    path[..1].to_lowercase(),
                    &path[2..]
                )
            } else {
                path
            }
        } else {
            host_workspace.clone()
        };

        let mut docker_args = vec![
            "run",
            "--rm",
            "--workdir", &work_dir,
            "--memory", &self.config.memory_limit,
        ];

        if self.config.read_only {
            docker_args.push("--read-only");
        }

        if !self.config.network_enabled {
            docker_args.push("--network");
            docker_args.push("none");
        }

        // 挂载工作目录
        let mount_volume = format!("{}:{}", host_workspace_docker, self.config.container_workspace);
        docker_args.push("-v");
        docker_args.push(&mount_volume);

        docker_args.push(&self.config.image);
        docker_args.push("sh");
        docker_args.push("-c");
        docker_args.push(command);

        log::info!(
            "[DockerSandbox] docker {}",
            docker_args.iter().map(|a| {
                if a.contains(' ') { format!("\"{}\"", a) } else { a.to_string() }
            }).collect::<Vec<_>>().join(" ")
        );

        let start = std::time::Instant::now();
        let output = TokioCommand::new("docker")
            .args(&docker_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Docker 执行失败: {}", e))?;

        let elapsed = start.elapsed();
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        log::info!(
            "[DockerSandbox] 退出码: {}, 耗时: {:.1}s, stdout: {} 字节, stderr: {} 字节",
            exit_code,
            elapsed.as_secs_f64(),
            stdout.len(),
            stderr.len()
        );

        Ok(SandboxResult {
            exit_code,
            stdout,
            stderr,
            sandboxed: true,
        })
    }

    /// 降级为本地执行
    async fn execute_locally(
        &self,
        command: &str,
        cwd: Option<&str>,
    ) -> Result<SandboxResult, String> {
        let shell = if cfg!(windows) { "powershell" } else { "sh" };
        let shell_flag = if cfg!(windows) { "-Command" } else { "-c" };

        let start = std::time::Instant::now();

        let mut cmd = TokioCommand::new(shell);
        cmd.arg(shell_flag)
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("本地执行失败: {}", e))?;

        let elapsed = start.elapsed();
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        log::info!(
            "[DockerSandbox:local] 退出码: {}, 耗时: {:.1}s",
            exit_code,
            elapsed.as_secs_f64()
        );

        Ok(SandboxResult {
            exit_code,
            stdout,
            stderr,
            sandboxed: false,
        })
    }
}
