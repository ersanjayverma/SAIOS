use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::kernel::testing::framework::{TestCase, TestSuite};
use crate::saifs;
use crate::saifs::{
    CreateKind, DirEntry, LookupContext, LookupResult, MountManager, NamespaceManager,
    NamespaceProvider, PathResolver, ProviderId, SaifsError, SaifsNodeKind,
};
use crate::{kt_assert, kt_assert_eq};

static TEST_ID: AtomicU64 = AtomicU64::new(1);

struct MockNamespaceProvider {
    id: ProviderId,
    name: String,
}

impl NamespaceProvider for MockNamespaceProvider {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn lookup(&self, _ctx: &LookupContext, path: &str) -> Result<LookupResult, SaifsError> {
        match path {
            "/" | "/beta" => Ok(LookupResult {
                object_id: None,
                kind: SaifsNodeKind::Directory,
            }),
            "/alpha" | "/beta/gamma" => Ok(LookupResult {
                object_id: None,
                kind: SaifsNodeKind::File,
            }),
            _ => Err(SaifsError::NotFound),
        }
    }

    fn enumerate(&self, _ctx: &LookupContext, path: &str) -> Result<Vec<DirEntry>, SaifsError> {
        let entries = match path {
            "/" => vec![
                DirEntry {
                    name: "beta".to_string(),
                    kind: SaifsNodeKind::Directory,
                },
                DirEntry {
                    name: "alpha".to_string(),
                    kind: SaifsNodeKind::File,
                },
            ],
            "/beta" => vec![DirEntry {
                name: "gamma".to_string(),
                kind: SaifsNodeKind::File,
            }],
            _ => return Err(SaifsError::NotFound),
        };

        Ok(entries)
    }

    fn create(
        &self,
        _ctx: &LookupContext,
        _path: &str,
        _kind: CreateKind,
    ) -> Result<crate::som::ObjectId, SaifsError> {
        Err(SaifsError::UnsupportedOperation)
    }

    fn remove(&self, _ctx: &LookupContext, _path: &str) -> Result<(), SaifsError> {
        Err(SaifsError::UnsupportedOperation)
    }
}

fn next_id() -> u64 {
    TEST_ID.fetch_add(1, Ordering::Relaxed)
}

fn register_mock_provider() -> Result<(ProviderId, u64), &'static str> {
    let token = next_id();
    let provider = Box::leak(Box::new(MockNamespaceProvider {
        id: ProviderId(10_000 + token),
        name: format!("mock-ns-{}", token),
    }));

    saifs::register_provider(provider)
        .map(|id| (id, token))
        .map_err(|_| "register mock provider failed")
}

fn test_saifs_initialized() -> Result<(), &'static str> {
    saifs::init();
    kt_assert!(saifs::is_initialized());
    Ok(())
}

fn test_saifs_root_mount_exists() -> Result<(), &'static str> {
    saifs::init();
    kt_assert!(saifs::mounts().iter().any(|m| m.path == "/"));
    Ok(())
}

fn test_path_resolver_canonicalize() -> Result<(), &'static str> {
    saifs::init();
    let resolved = saifs::path_resolver()
        .canonicalize("tmp/./saifs/../resolver")
        .map_err(|_| "canonicalize failed")?;
    kt_assert_eq!("/tmp/resolver", resolved.as_str());
    Ok(())
}

fn test_default_handle_read_write_roundtrip() -> Result<(), &'static str> {
    saifs::init();
    let token = next_id();
    let path = format!("/tmp/saifs-fw-{}", token);

    saifs::touch(&path).map_err(|_| "touch failed")?;

    let handle = saifs::open(&path).map_err(|_| "open failed")?;
    let bytes = b"saifs-framework";
    let written = crate::saifs::Handle::write(&handle, bytes).map_err(|_| "write failed")?;
    kt_assert_eq!(bytes.len(), written);

    let read_back = saifs::read_text(&path).map_err(|_| "read_text failed")?;
    kt_assert_eq!("saifs-framework", read_back.as_str());

    saifs::remove(&path).map_err(|_| "cleanup remove failed")?;
    Ok(())
}

fn test_mount_manager_resolves_longest_prefix() -> Result<(), &'static str> {
    saifs::init();
    let (provider_a, token) = register_mock_provider()?;
    let (provider_b, _) = register_mock_provider()?;

    let root_mount = format!("/mp-{}", token);
    let deep_mount = format!("{}/deep", root_mount);

    saifs::mount(&root_mount, provider_a, false).map_err(|_| "mount a failed")?;
    saifs::mount(&deep_mount, provider_b, false).map_err(|_| "mount b failed")?;

    let resolved = saifs::mount_manager()
        .resolve_provider(&format!("{}/beta/gamma", deep_mount))
        .map_err(|_| "resolve_provider failed")?;
    kt_assert_eq!(provider_b, resolved);
    Ok(())
}

fn test_namespace_manager_uses_mounted_provider() -> Result<(), &'static str> {
    saifs::init();
    let (provider, token) = register_mock_provider()?;
    let mount_path = format!("/ns-{}", token);
    saifs::mount(&mount_path, provider, false).map_err(|_| "mount failed")?;

    let handle = saifs::open(&format!("{}/alpha", mount_path)).map_err(|_| "open failed")?;
    kt_assert_eq!(provider, crate::saifs::Handle::provider_id(&handle));
    kt_assert_eq!("/alpha", handle.provider_path());

    let entries = saifs::list(&mount_path).map_err(|_| "list failed")?;
    kt_assert!(entries.len() == 2);
    kt_assert_eq!("alpha", entries[0].as_str());
    kt_assert_eq!("beta", entries[1].as_str());
    Ok(())
}

fn test_read_only_mount_blocks_mutation() -> Result<(), &'static str> {
    saifs::init();
    let (provider, token) = register_mock_provider()?;
    let mount_path = format!("/ro-{}", token);
    saifs::mount(&mount_path, provider, true).map_err(|_| "mount failed")?;

    let create_res =
        saifs::namespace_manager().create(&format!("{}/alpha", mount_path), CreateKind::File);
    kt_assert_eq!(Err(SaifsError::AccessDenied), create_res);

    let remove_res = saifs::remove(&format!("{}/alpha", mount_path));
    kt_assert_eq!(Err(SaifsError::AccessDenied), remove_res);
    Ok(())
}

fn test_unmount_restores_fallback_provider_and_emits_event() -> Result<(), &'static str> {
    saifs::init();
    let (provider, token) = register_mock_provider()?;
    let mount_path = format!("/um-{}", token);
    saifs::mount(&mount_path, provider, false).map_err(|_| "mount failed")?;

    let before = saifs::mount_manager()
        .resolve_provider(&format!("{}/alpha", mount_path))
        .map_err(|_| "resolve before failed")?;
    kt_assert_eq!(provider, before);

    saifs::unmount(&mount_path).map_err(|_| "unmount failed")?;

    let after = saifs::mount_manager()
        .resolve_provider(&format!("{}/alpha", mount_path))
        .map_err(|_| "resolve after failed")?;
    kt_assert!(after != provider);

    let recent = saifs::events(1);
    kt_assert!(matches!(
        recent.last().map(|e| e.event_type),
        Some(saifs::EventType::Unmounted)
    ));
    Ok(())
}

fn test_error_mapping_reports_unsupported_read() -> Result<(), &'static str> {
    saifs::init();
    let handle = saifs::open("/tmp").map_err(|_| "open /tmp failed")?;
    let res = crate::saifs::Handle::read(&handle);
    kt_assert_eq!(Err(SaifsError::UnsupportedOperation), res);
    Ok(())
}

const TESTS: [TestCase; 9] = [
    TestCase::new("saifs_initialized", test_saifs_initialized),
    TestCase::new("saifs_root_mount_exists", test_saifs_root_mount_exists),
    TestCase::new(
        "saifs_path_resolver_canonicalize",
        test_path_resolver_canonicalize,
    ),
    TestCase::new(
        "saifs_default_handle_read_write_roundtrip",
        test_default_handle_read_write_roundtrip,
    ),
    TestCase::new(
        "saifs_mount_manager_resolves_longest_prefix",
        test_mount_manager_resolves_longest_prefix,
    ),
    TestCase::new(
        "saifs_namespace_manager_uses_mounted_provider",
        test_namespace_manager_uses_mounted_provider,
    ),
    TestCase::new(
        "saifs_read_only_mount_blocks_mutation",
        test_read_only_mount_blocks_mutation,
    ),
    TestCase::new(
        "saifs_unmount_restores_fallback_provider",
        test_unmount_restores_fallback_provider_and_emits_event,
    ),
    TestCase::new(
        "saifs_error_mapping_reports_unsupported_read",
        test_error_mapping_reports_unsupported_read,
    ),
];

pub fn suite() -> TestSuite {
    TestSuite::new("saifs", &TESTS)
}
