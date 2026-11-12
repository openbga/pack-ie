//! What installing does besides registering the service: the state and log directories
//! become private, and the local operators group exists and has its members.

use std::path::{Path, PathBuf};

use crate::error::HostError;
use crate::runner::Runner;

/// `icacls` arguments that make `dir` private to the service (SEC-04): inheritance from
/// `C:\ProgramData` (which lets every user read) is removed and only SYSTEM and the
/// Administrators group keep full control, for the directory and everything created in
/// it. Well-known SIDs, so the names need no localization.
fn private_dir_acl(dir: &Path) -> Vec<String> {
    vec![
        dir.to_string_lossy().into_owned(),
        "/inheritance:r".to_owned(),
        "/grant:r".to_owned(),
        "*S-1-5-18:(OI)(CI)F".to_owned(),
        "*S-1-5-32-544:(OI)(CI)F".to_owned(),
        "/T".to_owned(),
        "/C".to_owned(),
        "/Q".to_owned(),
    ]
}

/// Makes the state and log directories private to the service.
pub(super) fn restrict_dirs(runner: &dyn Runner, dirs: &[&PathBuf]) -> Result<(), HostError> {
    for dir in dirs {
        let args = private_dir_acl(dir);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        runner.run_ok("icacls", &args)?;
    }
    Ok(())
}

/// The local group whose members may use the management pipe (`ipc.group` defaults to
/// it on Windows, see `packie-ipc`).
const OPERATORS_GROUP: &str = "PACK-IE Operators";
/// What `net localgroup` prints (in every language) when the group already exists:
/// `NET HELPMSG 2223` (`NERR_GroupExists`) or system error 1379 (`ERROR_ALIAS_EXISTS`).
const GROUP_EXISTS: [&str; 2] = ["2223", "1379"];
/// System error 1378 (`ERROR_MEMBER_IN_ALIAS`): the account is already a member.
const ALREADY_MEMBER: [&str; 1] = ["1378"];

/// Runs `net` with `args`, treating a failure whose output mentions one of the
/// `tolerated` numbers as success. `net` exits with 2 for every error and words its
/// messages in the system language, so the error number is what identifies the case.
fn net(runner: &dyn Runner, args: &[&str], tolerated: &[&str]) -> Result<(), HostError> {
    match runner.run_ok("net", args) {
        Err(HostError::Command { message, .. })
            if tolerated.iter().any(|number| message.contains(number)) =>
        {
            Ok(())
        }
        other => other,
    }
}

/// Creates the [`OPERATORS_GROUP`] when it is missing and adds `operator`
/// (`DOMAIN\user`) to it. Existing groups and memberships are left as they are.
pub(super) fn ensure_operators_group(
    runner: &dyn Runner,
    operator: Option<&str>,
) -> Result<(), HostError> {
    net(
        runner,
        &[
            "localgroup",
            OPERATORS_GROUP,
            "/add",
            "/comment:Positive ACK operators",
        ],
        &GROUP_EXISTS,
    )?;
    if let Some(operator) = operator {
        net(
            runner,
            &["localgroup", OPERATORS_GROUP, operator, "/add"],
            &ALREADY_MEMBER,
        )?;
    }
    Ok(())
}

/// The account to add to the operators group: the one the caller names
/// (`ServiceSpec::operator`), else the account of the elevated installer process
/// (`USERDOMAIN` and `USERNAME`). An elevated prompt opened as another administrator
/// therefore adds that administrator. Machine accounts (`NAME$`, as under `LocalSystem`)
/// are not people and are skipped.
pub(super) fn operator_account(
    configured: Option<&str>,
    domain: Option<&str>,
    user: Option<&str>,
) -> Option<String> {
    if let Some(name) = configured.filter(|name| !name.is_empty()) {
        return Some(name.to_owned());
    }
    let user = user.filter(|name| !name.is_empty() && !name.ends_with('$'))?;
    Some(match domain.filter(|domain| !domain.is_empty()) {
        Some(domain) => format!(r"{domain}\{user}"),
        None => user.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeRunner;

    #[test]
    fn state_and_log_directories_are_made_private() {
        let runner = FakeRunner::new(true);
        let state = PathBuf::from(r"C:\ProgramData\Positive ACK\state");
        let logs = PathBuf::from(r"C:\ProgramData\Positive ACK\logs");
        restrict_dirs(&runner, &[&state, &logs]).unwrap();
        assert_eq!(
            runner.calls(),
            [
                r"icacls C:\ProgramData\Positive ACK\state /inheritance:r /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F /T /C /Q",
                r"icacls C:\ProgramData\Positive ACK\logs /inheritance:r /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F /T /C /Q",
            ]
        );
    }

    fn output_of(runner: &FakeRunner, code: i32, stderr: &str) {
        runner.push_output(code, "", stderr);
    }

    #[test]
    fn the_operators_group_is_created_and_the_operator_added() {
        let runner = FakeRunner::new(true);
        ensure_operators_group(&runner, Some(r"LABPC\alice")).unwrap();
        assert_eq!(
            runner.calls(),
            [
                "net localgroup PACK-IE Operators /add /comment:Positive ACK operators",
                r"net localgroup PACK-IE Operators LABPC\alice /add",
            ]
        );
    }

    #[test]
    fn without_an_operator_only_the_group_is_created() {
        let runner = FakeRunner::new(true);
        ensure_operators_group(&runner, None).unwrap();
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn an_existing_group_and_membership_are_tolerated() {
        for exists in [
            "The specified local group already exists.\r\n\r\nMore help is available by typing NET HELPMSG 2223.",
            "System error 1379 has occurred.",
            // Another language: only the number is the same.
            "Der angegebene lokale Gruppenname ist bereits vorhanden. NET HELPMSG 2223.",
        ] {
            let runner = FakeRunner::new(true);
            output_of(&runner, 2, exists);
            output_of(&runner, 2, "System error 1378 has occurred.\r\n");
            ensure_operators_group(&runner, Some("alice")).unwrap();
            assert_eq!(runner.calls().len(), 2, "{exists}");
        }
    }

    #[test]
    fn other_failures_are_reported() {
        let runner = FakeRunner::new(true);
        output_of(&runner, 2, "System error 5 has occurred. Access is denied.");
        let error = ensure_operators_group(&runner, Some("alice")).unwrap_err();
        assert!(error.to_string().contains("Access is denied"), "{error}");
        assert_eq!(runner.calls().len(), 1, "stops at the first failure");

        let runner = FakeRunner::new(true);
        output_of(&runner, 0, "");
        output_of(
            &runner,
            2,
            "System error 1387 has occurred. No such member.",
        );
        let error = ensure_operators_group(&runner, Some("nobody")).unwrap_err();
        assert!(error.to_string().contains("1387"), "{error}");

        // A group-exists number does not excuse a failed membership, and vice versa.
        let runner = FakeRunner::new(true);
        output_of(&runner, 0, "");
        output_of(&runner, 2, "NET HELPMSG 2223");
        assert!(ensure_operators_group(&runner, Some("alice")).is_err());
        let runner = FakeRunner::new(true);
        output_of(&runner, 2, "System error 1378 has occurred.");
        assert!(ensure_operators_group(&runner, Some("alice")).is_err());
    }

    #[test]
    fn the_operator_is_the_named_account_or_the_installing_one() {
        assert_eq!(
            operator_account(Some(r"LAB\bob"), Some("PC"), Some("alice")).as_deref(),
            Some(r"LAB\bob")
        );
        assert_eq!(
            operator_account(None, Some("LABPC"), Some("alice")).as_deref(),
            Some(r"LABPC\alice")
        );
        assert_eq!(
            operator_account(Some(""), None, Some("alice")).as_deref(),
            Some("alice")
        );
        assert_eq!(operator_account(None, Some("PC"), Some("")), None);
        assert_eq!(
            operator_account(None, Some("WORKGROUP"), Some("LABPC$")),
            None
        );
        assert_eq!(operator_account(None, None, None), None);
    }
}
