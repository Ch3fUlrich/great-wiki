use crate::principal::Principal;
use gw_core::Visibility;
use serde::{Deserialize, Serialize};

/// What a caller is trying to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Read,
    Comment,
    Write,
    Admin,
}

/// What a grant confers. Ordered, so a stronger grant satisfies a weaker action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Read,
    Comment,
    Write,
    Admin,
}

impl Permission {
    fn satisfies(self, action: Action) -> bool {
        let level = |p| match p {
            Permission::Read => 0,
            Permission::Comment => 1,
            Permission::Write => 2,
            Permission::Admin => 3,
        };
        let needed = match action {
            Action::Read => 0,
            Action::Comment => 1,
            Action::Write => 2,
            Action::Admin => 3,
        };
        level(self) >= needed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum Subject {
    Principal(String),
    Team(String),
    /// An OIDC group from the verified `groups` claim.
    Group(String),
    /// Everyone, including anonymous callers. Used by public share links.
    Anyone,
    /// Any signed-in principal.
    Authenticated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub subject: Subject,
    pub permission: Permission,
}

/// The single authorisation decision in the system.
///
/// Every handler, every retriever and every export path calls this. Nothing else decides.
///
/// The ordering below is load-bearing. Authentication is checked BEFORE group and team
/// membership, because those lists are only meaningful once the caller is known — an
/// anonymous request carrying a forged group must fail, and the test
/// `anonymous_never_passes_a_grant_gate_even_with_a_matching_name` proves it.
pub fn can(
    principal: &Principal,
    action: Action,
    visibility: Visibility,
    grants: &[Grant],
) -> bool {
    // Public reads are the one thing that survives everything, including deactivation:
    // suspending an account must not make the public site disappear for that person.
    if action == Action::Read && visibility == Visibility::Public {
        return true;
    }

    // An `Anyone` grant is the only way an anonymous caller gets past this point. It is
    // how a public share link works, and it confers exactly what it says.
    let anyone = grants
        .iter()
        .filter(|g| g.subject == Subject::Anyone)
        .any(|g| g.permission.satisfies(action));
    if anyone {
        return true;
    }

    if !principal.is_authenticated() || !principal.active {
        return false;
    }

    if action == Action::Read && visibility == Visibility::Internal {
        return true;
    }

    grants.iter().any(|grant| {
        let matches = match &grant.subject {
            Subject::Principal(id) => *id == principal.id || *id == principal.username,
            Subject::Team(t) => principal.teams.iter().any(|mine| mine == t),
            Subject::Group(g) => principal.groups.iter().any(|mine| mine == g),
            Subject::Authenticated => true,
            Subject::Anyone => false, // already handled above
        };
        matches && grant.permission.satisfies(action)
    })
}

#[cfg(test)]
mod tests {
    use crate::permission::{can, Action, Grant, Permission, Subject};
    use crate::principal::Principal;
    use gw_core::Visibility;

    fn guest() -> Principal {
        Principal::test("guest", &[], &[])
    }
    fn member() -> Principal {
        Principal::test("member", &["users"], &["editors"])
    }
    fn anon() -> Principal {
        Principal::anonymous()
    }

    #[test]
    fn public_documents_are_readable_by_anyone_including_anonymous() {
        assert!(can(&anon(), Action::Read, Visibility::Public, &[]));
    }

    #[test]
    fn public_documents_are_not_writable_without_a_grant() {
        // Public means readable, never editable. Conflating the two is how wikis get defaced.
        assert!(!can(&anon(), Action::Write, Visibility::Public, &[]));
        assert!(!can(&guest(), Action::Write, Visibility::Public, &[]));
    }

    #[test]
    fn internal_documents_need_authentication_but_no_grant() {
        assert!(!can(&anon(), Action::Read, Visibility::Internal, &[]));
        assert!(can(&guest(), Action::Read, Visibility::Internal, &[]));
    }

    #[test]
    fn restricted_documents_need_a_matching_grant() {
        assert!(!can(&guest(), Action::Read, Visibility::Restricted, &[]));
    }

    #[test]
    fn internal_reach_confers_reading_and_nothing_else() {
        // Being signed in with internal reach lets you READ an internal document. It must
        // not let you change one: writing is only ever conferred by an explicit grant.
        //
        // Found by mutation testing, not by review. Deleting `action == Action::Read` from
        // the internal branch — which hands write, comment AND admin on every internal
        // document to everyone who can see it — failed no test in the suite.
        for action in [Action::Comment, Action::Write, Action::Admin] {
            assert!(
                !can(&guest(), action, Visibility::Internal, &[]),
                "internal reach conferred {action:?} with no grant"
            );
        }
        assert!(can(&guest(), Action::Read, Visibility::Internal, &[]));
    }

    #[test]
    fn a_public_document_is_readable_but_not_writable() {
        // The same shape one visibility down, and the same failure if the read check is
        // dropped: a public page would become world-writable.
        for action in [Action::Comment, Action::Write, Action::Admin] {
            assert!(
                !can(&anon(), action, Visibility::Public, &[]),
                "a public document accepted {action:?} from an anonymous caller"
            );
        }
        assert!(can(&anon(), Action::Read, Visibility::Public, &[]));
        let grants = [Grant {
            subject: Subject::Principal("guest".into()),
            permission: Permission::Read,
        }];
        assert!(can(&guest(), Action::Read, Visibility::Restricted, &grants));
    }

    #[test]
    fn a_team_grant_reaches_its_members() {
        let grants = [Grant {
            subject: Subject::Team("editors".into()),
            permission: Permission::Write,
        }];
        assert!(can(
            &member(),
            Action::Write,
            Visibility::Restricted,
            &grants
        ));
        assert!(!can(
            &guest(),
            Action::Write,
            Visibility::Restricted,
            &grants
        ));
    }

    #[test]
    fn an_oidc_group_grant_reaches_its_members() {
        let grants = [Grant {
            subject: Subject::Group("users".into()),
            permission: Permission::Read,
        }];
        assert!(can(
            &member(),
            Action::Read,
            Visibility::Restricted,
            &grants
        ));
        assert!(!can(
            &guest(),
            Action::Read,
            Visibility::Restricted,
            &grants
        ));
    }

    #[test]
    fn stronger_permissions_imply_weaker_actions() {
        let grants = [Grant {
            subject: Subject::Principal("guest".into()),
            permission: Permission::Admin,
        }];
        for action in [Action::Read, Action::Comment, Action::Write, Action::Admin] {
            assert!(can(&guest(), action, Visibility::Restricted, &grants));
        }
    }

    #[test]
    fn weaker_permissions_do_not_imply_stronger_actions() {
        let grants = [Grant {
            subject: Subject::Principal("guest".into()),
            permission: Permission::Read,
        }];
        assert!(can(&guest(), Action::Read, Visibility::Restricted, &grants));
        assert!(!can(
            &guest(),
            Action::Comment,
            Visibility::Restricted,
            &grants
        ));
        assert!(!can(
            &guest(),
            Action::Write,
            Visibility::Restricted,
            &grants
        ));
        assert!(!can(
            &guest(),
            Action::Admin,
            Visibility::Restricted,
            &grants
        ));
    }

    #[test]
    fn anonymous_never_passes_a_grant_gate_even_with_a_matching_name() {
        // A forged identity claiming a group must not pass: authentication is checked
        // BEFORE group membership, because groups are only meaningful once a user is known.
        let mut forged = Principal::anonymous();
        forged.groups = vec!["admins".into()];
        forged.teams = vec!["editors".into()];
        let grants = [
            Grant {
                subject: Subject::Group("admins".into()),
                permission: Permission::Admin,
            },
            Grant {
                subject: Subject::Team("editors".into()),
                permission: Permission::Admin,
            },
            Grant {
                subject: Subject::Authenticated,
                permission: Permission::Admin,
            },
        ];
        assert!(!can(&forged, Action::Read, Visibility::Restricted, &grants));
    }

    #[test]
    fn a_deactivated_principal_is_denied_everything_but_public_reads() {
        let mut suspended = member();
        suspended.active = false;
        let grants = [Grant {
            subject: Subject::Team("editors".into()),
            permission: Permission::Admin,
        }];
        assert!(!can(
            &suspended,
            Action::Read,
            Visibility::Restricted,
            &grants
        ));
        assert!(!can(&suspended, Action::Read, Visibility::Internal, &[]));
        // Deactivating someone must not make public pages unreadable to them.
        assert!(can(&suspended, Action::Read, Visibility::Public, &[]));
    }

    #[test]
    fn anyone_grants_reach_anonymous_but_only_for_the_granted_permission() {
        let grants = [Grant {
            subject: Subject::Anyone,
            permission: Permission::Read,
        }];
        assert!(can(&anon(), Action::Read, Visibility::Restricted, &grants));
        assert!(!can(
            &anon(),
            Action::Write,
            Visibility::Restricted,
            &grants
        ));
    }
}
