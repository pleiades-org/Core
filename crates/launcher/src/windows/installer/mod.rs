//! Current-user setup and removal, rendered with Core's existing native UI.
mod files;
mod install;
mod metadata;
mod options;
mod paths;
mod payload;
mod registry;
mod shortcuts;
mod temporary;
mod ui;

/// Handles setup before Core acquires its normal single-instance lock or starts workers.
pub fn dispatch() -> Result<bool, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let explicit_operation = arguments.iter().any(|argument| {
        matches!(
            argument.to_str(),
            Some("--install" | "--uninstall" | "--installer-preview")
        )
    });
    if !explicit_operation && payload::is_setup(&executable)? {
        arguments.push("--install".into());
    }
    let Some(options) = options::Options::parse(&arguments, &executable)? else {
        return Ok(false);
    };
    let paths = paths::InstallPaths::new(options.test_root.as_deref())?;
    if options.operation == options::Operation::Uninstall
        && [&paths.executable, &paths.uninstaller]
            .into_iter()
            .any(|installed| {
                std::fs::canonicalize(installed)
                    .ok()
                    .zip(std::fs::canonicalize(&executable).ok())
                    .is_some_and(|(installed, running)| installed == running)
            })
    {
        temporary::launch_uninstaller(&executable, &arguments)?;
        return Ok(true);
    }
    let result = ui::run(options, paths);
    temporary::cleanup(&executable);
    result.map(|()| true)
}

pub(super) fn refresh_registration() {
    let outcome = (|| {
        let paths = paths::InstallPaths::new(None)?;
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        if !paths.executable.exists()
            || std::fs::canonicalize(&executable).map_err(|error| error.to_string())?
                != std::fs::canonicalize(&paths.executable).map_err(|error| error.to_string())?
        {
            return Ok(());
        }
        metadata::refresh(&paths)
    })();
    if let Err(error) = outcome {
        eprintln!("Could not refresh Core's installed version in Windows: {error}");
    }
}
