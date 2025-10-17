use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Absolute path to the nix-eval-jobs executable
///
/// We expect this environment to be set in Nix build and shell.
pub const NIX_EVAL_JOBS: &str = env!("NIX_EVAL_JOBS");

#[derive(Debug, Serialize, Deserialize)]
pub struct NixEvalJob {
    pub attr: String,
    #[serde(rename = "attrPath")]
    pub attr_path: Vec<String>,
    #[serde(rename = "drvPath")]
    pub drv_path: String,
    pub name: String,
    pub outputs: HashMap<String, String>,
    pub system: String,
}

/// The `nix-eval-jobs` command
/// See documentation for [nix-eval-jobs](https://github.com/nix-community/nix-eval-jobs)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct NixEvalJobsCmd;

impl NixEvalJobsCmd {
    pub fn command(&self) -> Command {
        let nix_eval_jobs = env!("NIX_EVAL_JOBS").to_string();
        let mut cmd = Command::new(nix_eval_jobs);
        cmd.kill_on_drop(true);
        cmd
    }
}

impl NixEvalJobsCmd {
    pub async fn run_nix_eval_jobs(
        &self,
        flake_ref: &str,
        extra_nix_build_args: Vec<String>,
    ) -> Result<Vec<NixEvalJob>> {
        tracing::info!("{}", "🍏 Running Evaluation".to_string().bold());
        let mut cmd = self.command();
        cmd.args(["--flake", flake_ref]);
        cmd.args(&extra_nix_build_args);

        nix_rs::command::trace_cmd(&cmd);
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn nix-eval-jobs")?;

        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        let mut results = Vec::new();

        while let Some(line) = lines.next_line().await.context("Failed to read line")? {
            // Skip warning lines and empty lines
            if line.trim().is_empty()
                || line.starts_with("warning:")
                || line.starts_with("evaluation warning:")
            {
                continue;
            }

            // Try to parse as JSON
            match serde_json::from_str::<NixEvalJob>(&line) {
                Ok(job) => results.push(job),
                Err(e) => {
                    eprintln!("Failed to parse line as JSON: {}", e);
                    eprintln!("Line: {}", line);
                }
            }
        }

        let status = child
            .wait()
            .await
            .context("Failed to wait for child process")?;

        if !status.success() {
            anyhow::bail!("nix-eval-jobs exited with status: {}", status);
        }

        Ok(results)
    }
}
