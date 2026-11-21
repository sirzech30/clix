use std::fs::OpenOptions;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PermissionStatus {
    pub uinput_accessible: bool,
    pub input_group_member: bool,
    pub message: String,
}

pub fn check_permissions() -> PermissionStatus {
    let uinput_accessible = can_access_uinput();
    let input_group_member = is_in_input_group();

    let message = if uinput_accessible {
        "uinput access OK".to_string()
    } else if input_group_member {
        "Cannot access /dev/uinput. Install udev rules and re-login.".to_string()
    } else {
        "Add your user to the 'input' group and install udev rules (see README).".to_string()
    };

    PermissionStatus {
        uinput_accessible,
        input_group_member,
        message,
    }
}

fn can_access_uinput() -> bool {
    for path in ["/dev/uinput", "/dev/input/uinput"] {
        if Path::new(path).exists() {
            return OpenOptions::new().read(true).write(true).open(path).is_ok();
        }
    }
    false
}

fn is_in_input_group() -> bool {
    let uid = nix::unistd::getuid();
    let Ok(Some(group)) = nix::unistd::Group::from_name("input") else {
        return false;
    };

    if let Ok(Some(user)) = nix::unistd::User::from_uid(uid) {
        if user.gid == group.gid {
            return true;
        }
        if group.mem.iter().any(|m| m == &user.name) {
            return true;
        }
    }

    if let Ok(groups) = nix::unistd::getgroups()
        && groups.contains(&group.gid)
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_permissions_returns_consistent_message() {
        let status = check_permissions();
        assert!(!status.message.is_empty());
    }
}
