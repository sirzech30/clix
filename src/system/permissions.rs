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
            return OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .is_ok();
        }
    }
    false
}

fn is_in_input_group() -> bool {
    let uid = nix::unistd::getuid();
    if let Ok(Some(group)) = nix::unistd::Group::from_name("input") {
        if let Ok(Some(user)) = nix::unistd::User::from_uid(uid) {
            if user.gid == group.gid {
                return true;
            }
        }
    }

    std::fs::read_to_string("/etc/group")
        .map(|content| {
            let uid_str = uid.as_raw().to_string();
            content.lines().any(|line| {
                let mut parts = line.split(':');
                let name = parts.next().unwrap_or_default();
                let _gid = parts.next();
                let members = parts.next().unwrap_or_default();
                name == "input" && members.split(',').any(|m| m == uid_str)
            })
        })
        .unwrap_or(false)
}
