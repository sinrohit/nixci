pub mod cli;
pub mod config;
pub mod github;
pub mod logging;
pub mod nix;

use anyhow::{Context, Ok};
use clap::CommandFactory;
use clap_complete::generate;
use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use cli::{BuildConfig, CliArgs};
use nix::{
    eval::NixEvalJobsCmd,
    nix_store::{DrvOut, NixStoreCmd, StorePath},
};
use nix_health::{traits::Checkable, NixHealth};
use nix_rs::{
    config::NixConfig,
    flake::{system::System, url::FlakeUrl},
    info::NixInfo,
};
use tracing::instrument;

/// Run nixci on the given [CliArgs], returning the built outputs in sorted order.
#[instrument(name = "nixci", skip(args))]
pub async fn nixci(args: CliArgs) -> anyhow::Result<Vec<StorePath>> {
    tracing::debug!("Args: {args:?}");

    match args.command {
        cli::Command::Build(build_cfg) => {
            let flake_url = cli::Command::get_flake_url(&build_cfg.flake_ref).await?;
            let nix_info = NixInfo::from_nix(&args.nixcmd)
                .await
                .with_context(|| "Unable to gather nix info")?;
            nixci_build(&build_cfg, flake_url, &nix_info.nix_config).await
        }
        cli::Command::DumpGithubActionsMatrix {
            systems, flake_ref, ..
        } => {
            let cfg = cli::Command::get_config(&args.nixcmd, &flake_ref).await?;
            let matrix = github::matrix::GitHubMatrix::from(systems, &cfg.subflakes);
            println!("{}", serde_json::to_string(&matrix)?);
            Ok(vec![])
        }
        cli::Command::Completion { shell } => {
            let mut cli = CliArgs::command();
            let name = cli.get_name().to_string();
            generate(shell, &mut cli, name, &mut io::stdout());
            Ok(vec![])
        }
    }
}

fn get_flake_to_build(url: FlakeUrl, current_system: &System) -> anyhow::Result<FlakeUrl> {
    let (flake_url, attr) = url.split_attr();
    let nested_attr = attr.as_list();

    let result_url = match nested_attr.as_slice() {
        // Case 1: `.#checks` -> `.#checks.current_system`
        [name] => FlakeUrl(format!("{}#{}.{}", flake_url.0, name, current_system)),
        // Case 2 & 3: `.#checks.x86_64-linux` or `.#packages.aarch64-darwin.default`
        // Return as-is since system is already specified
        [_, system_or_more @ ..] if !system_or_more.is_empty() => {
            url.clone() // Return the original URL unchanged
        }
        [] => FlakeUrl(format!("{}#checks.{}", flake_url.0, current_system)),
        _ => anyhow::bail!("Invalid flake URL: {}", url.0),
    };

    Ok(result_url)
}

async fn nixci_build(
    build_cfg: &BuildConfig,
    flake_url: FlakeUrl,
    nix_config: &NixConfig,
) -> anyhow::Result<Vec<StorePath>> {
    let flake = get_flake_to_build(flake_url, &nix_config.system.value)?;
    let jobs = NixEvalJobsCmd
        .run_nix_eval_jobs(&flake.0, build_cfg.extra_nix_build_args.clone())
        .await?;

    tracing::info!("🍎 Evaluation Complete!");
    tracing::info!("⏱️ Scheduling {} Builds", jobs.len());

    let mut result = Vec::<DrvOut>::new();

    for job in &jobs {
        tracing::info!("🛠️ Building {} for {}", job.attr, job.system);
        let drv_out = DrvOut(PathBuf::from(job.drv_path.clone()));
        let out = NixStoreCmd.nix_store_realise(drv_out).await?;
        result.push(out);
    }
    let mut all_outs = HashSet::new();

    if build_cfg.print_all_dependencies {
        let all_deps = NixStoreCmd
            .fetch_all_deps(result.into_iter().collect())
            .await?;
        all_outs.extend(all_deps.into_iter());
    } else {
        let store_paths: HashSet<StorePath> =
            result.into_iter().map(DrvOut::as_store_path).collect();
        all_outs.extend(store_paths);
    }

    for out in &all_outs {
        println!("{}", out);
    }

    Ok(all_outs.into_iter().collect())
}

pub async fn check_nix_version(flake_url: &FlakeUrl, nix_info: &NixInfo) -> anyhow::Result<()> {
    let nix_health = NixHealth::from_flake(flake_url).await?;
    let checks = nix_health.nix_version.check(nix_info, Some(flake_url));
    let exit_code = NixHealth::print_report_returning_exit_code(&checks);

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}
