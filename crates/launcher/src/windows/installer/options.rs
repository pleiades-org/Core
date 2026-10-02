use std::{ffi::OsString, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Install,
    Uninstall,
}

#[derive(Clone, Debug)]
pub(super) struct Options {
    pub operation: Operation,
    pub preview: bool,
    pub background: bool,
    pub test_root: Option<std::path::PathBuf>,
}

impl Options {
    pub fn parse(arguments: &[OsString], executable: &Path) -> Result<Option<Self>, String> {
        let mut operation = None;
        let mut preview = false;
        let mut background = false;
        let mut test_root = None;
        let mut arguments = arguments.iter();
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--install") => set_operation(&mut operation, Operation::Install)?,
                Some("--uninstall") => set_operation(&mut operation, Operation::Uninstall)?,
                Some("--installer-preview") => {
                    preview = true;
                    set_operation(&mut operation, Operation::Install)?;
                }
                Some("--test-background") => background = true,
                Some("--installer-test-root") => {
                    if test_root.is_some() {
                        return Err("The installer test directory was specified twice.".into());
                    }
                    test_root = Some(
                        arguments
                            .next()
                            .filter(|path| !path.is_empty())
                            .ok_or("--installer-test-root requires an isolated directory.")?
                            .into(),
                    );
                }
                _ => {}
            }
        }
        if operation.is_none() && is_setup_name(executable) {
            operation = Some(Operation::Install);
        }
        if operation.is_none()
            && executable
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("uninstall.exe"))
        {
            operation = Some(Operation::Uninstall);
        }
        let Some(operation) = operation else {
            if test_root.is_some() {
                return Err(
                    "An installer test directory requires --install or --uninstall.".into(),
                );
            }
            return Ok(None);
        };
        Ok(Some(Self {
            operation,
            preview,
            background,
            test_root,
        }))
    }
}

fn set_operation(current: &mut Option<Operation>, next: Operation) -> Result<(), String> {
    if current.is_some_and(|operation| operation != next) {
        return Err("Choose either installation or uninstallation.".into());
    }
    *current = Some(next);
    Ok(())
}

fn is_setup_name(executable: &Path) -> bool {
    let Some(name) = executable.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    let Some(version) = name
        .strip_prefix("core-setup-")
        .and_then(|name| name.strip_suffix(".exe"))
    else {
        return false;
    };
    version.parse::<crate::windows::updates::Version>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_packaged_setup_name_automatically_opens_installation() {
        for name in ["Core-Setup-2.3.0.exe", "CORE-SETUP-2.3.1.EXE"] {
            assert_eq!(
                Options::parse(&[], Path::new(name))
                    .unwrap()
                    .unwrap()
                    .operation,
                Operation::Install
            );
        }
        for name in [
            "core-v2.exe",
            "Core-Setup.exe",
            "Core-Setup-2.3.exe",
            "Core-Setup-2.3.0.exe.old",
        ] {
            assert!(Options::parse(&[], Path::new(name)).unwrap().is_none());
        }
    }

    #[test]
    fn conflicting_operations_and_incomplete_test_arguments_are_rejected() {
        for arguments in [
            vec!["--install", "--uninstall"],
            vec!["--install", "--installer-test-root"],
            vec!["--installer-test-root", "example"],
        ] {
            let arguments: Vec<OsString> = arguments.into_iter().map(OsString::from).collect();
            assert!(Options::parse(&arguments, Path::new("core-v2.exe")).is_err());
        }
    }

    #[test]
    fn installed_uninstaller_opens_removal_without_arguments() {
        assert_eq!(
            Options::parse(&[], Path::new("uninstall.exe"))
                .unwrap()
                .unwrap()
                .operation,
            Operation::Uninstall
        );
    }
}
