use prdb_engine::sdk_registry::{SdkRegistryError, SdkReleaseRegistry};
use prdb_engine::submission_manifest::ArtifactManifestV1;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn temp_dir() -> PathBuf {
    let mut random = [0u8; 12];
    getrandom::fill(&mut random).unwrap();
    let suffix = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    std::env::temp_dir().join(format!("prdb-sdk-registry-{}-{suffix}", std::process::id()))
}

fn built_release(root: &Path) -> PathBuf {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let build_dir = root.join("sdk");
    let output = Command::new("make")
        .current_dir(workspace)
        .args([
            "-f",
            "engine/sdk/Makefile",
            &format!("BUILD_DIR={}", build_dir.display()),
            "SDK_RUNTIME_PROFILE=test-runtime",
            "sdk-shared-release",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    build_dir.join("releases/linux-x86_64/x86_64-v1/test-runtime/sdk-1-abis1,2,3,4,5")
}

fn manifest(abi: u8, release: u32) -> ArtifactManifestV1 {
    ArtifactManifestV1 {
        native_abi_version: abi,
        sdk_release: release,
        cpu_baseline: "x86_64-v1".to_owned(),
        runtime_profile: "test-runtime".to_owned(),
        compiler_identity: "registry integration test".to_owned(),
        descriptor_symbol: format!("prdb_query_descriptor_v{abi}"),
        entry_point_symbol: format!("prdb_query_main_v{abi}"),
        dependencies: vec!["libc.so.6".to_owned(), format!("libprdb_sql.so.{release}")],
    }
}

fn load(releases: &[PathBuf], cache: &Path, max_bytes: u64) -> SdkReleaseRegistry {
    SdkReleaseRegistry::load(
        releases,
        cache,
        "linux-x86_64".to_owned(),
        "x86_64-v1".to_owned(),
        "test-runtime".to_owned(),
        max_bytes,
    )
    .unwrap()
}

#[test]
fn registry_validates_and_atomically_caches_a_release_then_resolves_compatibility() {
    let root = temp_dir();
    fs::create_dir(&root).unwrap();
    let release = built_release(&root);
    let cache = root.join("cache");
    let registry = load(std::slice::from_ref(&release), &cache, 128 * 1024 * 1024);
    let pin = registry.resolve(&manifest(4, 1)).unwrap();
    assert_eq!(pin.soname(), "libprdb_sql.so.1");
    assert!(pin.library_path().is_file());
    assert_eq!(
        registry.cache_bytes().unwrap(),
        fs::metadata(pin.library_path()).unwrap().len()
    );
    assert_eq!(
        registry.resolve(&manifest(4, 2)).unwrap_err(),
        SdkRegistryError::UnavailableRelease
    );
    let mut wrong_profile = manifest(4, 1);
    wrong_profile.cpu_baseline = "x86_64-v2".to_owned();
    assert_eq!(
        registry.resolve(&wrong_profile).unwrap_err(),
        SdkRegistryError::IncompatibleProfile
    );
    assert_eq!(registry.evict_unpinned().unwrap(), 0);
    let cached_path = pin.library_path().to_path_buf();
    drop(pin);
    assert!(registry.evict_unpinned().unwrap() > 0);
    assert!(!cached_path.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn registry_detects_corruption_profile_mismatch_and_cache_limits() {
    let root = temp_dir();
    fs::create_dir(&root).unwrap();
    let release = built_release(&root);

    let too_small_cache = root.join("too-small");
    assert_eq!(
        SdkReleaseRegistry::load(
            std::slice::from_ref(&release),
            &too_small_cache,
            "linux-x86_64".to_owned(),
            "x86_64-v1".to_owned(),
            "test-runtime".to_owned(),
            1,
        )
        .unwrap_err(),
        SdkRegistryError::CacheFull
    );
    assert_eq!(fs::read_dir(&too_small_cache).unwrap().count(), 0);

    let wrong_profile_cache = root.join("wrong-profile");
    assert_eq!(
        SdkReleaseRegistry::load(
            std::slice::from_ref(&release),
            &wrong_profile_cache,
            "linux-x86_64".to_owned(),
            "x86_64-v2".to_owned(),
            "test-runtime".to_owned(),
            128 * 1024 * 1024,
        )
        .unwrap_err(),
        SdkRegistryError::IncompatibleProfile
    );

    let corrupt_release = root.join("corrupt-release");
    fs::create_dir(&corrupt_release).unwrap();
    for name in ["sdk-release.json", "libprdb_sql.so.1"] {
        fs::copy(release.join(name), corrupt_release.join(name)).unwrap();
    }
    fs::write(corrupt_release.join("libprdb_sql.so.1"), b"corrupt").unwrap();
    assert_eq!(
        SdkReleaseRegistry::load(
            &[corrupt_release],
            &root.join("corrupt-cache"),
            "linux-x86_64".to_owned(),
            "x86_64-v1".to_owned(),
            "test-runtime".to_owned(),
            128 * 1024 * 1024,
        )
        .unwrap_err(),
        SdkRegistryError::InvalidMetadata
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_resolution_reuses_one_immutable_cache_entry_and_rechecks_digest() {
    let root = temp_dir();
    fs::create_dir(&root).unwrap();
    let release = built_release(&root);
    let cache = root.join("cache");
    let registry = Arc::new(load(
        std::slice::from_ref(&release),
        &cache,
        128 * 1024 * 1024,
    ));
    let workers = (0..8)
        .map(|_| {
            let registry = Arc::clone(&registry);
            std::thread::spawn(move || registry.resolve(&manifest(4, 1)).unwrap())
        })
        .collect::<Vec<_>>();
    let pins = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(
        pins.iter()
            .all(|pin| pin.library_path() == pins[0].library_path())
    );
    let path = pins[0].library_path().to_path_buf();
    drop(pins);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, b"tampered").unwrap();
    assert_eq!(
        registry.resolve(&manifest(4, 1)).unwrap_err(),
        SdkRegistryError::DigestMismatch
    );
    fs::set_permissions(path.parent().unwrap(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_registry_loads_publish_one_complete_cache_entry() {
    let root = temp_dir();
    fs::create_dir(&root).unwrap();
    let release = built_release(&root);
    let cache = root.join("cache");
    let loaders = (0..8)
        .map(|_| {
            let release = release.clone();
            let cache = cache.clone();
            std::thread::spawn(move || {
                let registry = load(&[release], &cache, 128 * 1024 * 1024);
                registry
                    .resolve(&manifest(4, 1))
                    .unwrap()
                    .library_path()
                    .to_path_buf()
            })
        })
        .collect::<Vec<_>>();
    let paths = loaders
        .into_iter()
        .map(|loader| loader.join().unwrap())
        .collect::<Vec<_>>();
    assert!(paths.iter().all(|path| path == &paths[0]));
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 1);
    let digest_dir = paths[0].parent().unwrap();
    assert_eq!(fs::read_dir(digest_dir).unwrap().count(), 1);
    fs::set_permissions(digest_dir, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repository_install_is_durable_listable_and_pin_safe_for_removal() {
    let root = temp_dir();
    fs::create_dir(&root).unwrap();
    let release = built_release(&root);
    let metadata = fs::read(release.join("sdk-release.json")).unwrap();
    let library = fs::read(release.join("libprdb_sql.so.1")).unwrap();
    let repository_root = root.join("instance").join("sdk-libraries");
    let allowed = HashSet::from(["libc.so.6".to_owned()]);
    let repository = SdkReleaseRegistry::open(
        &repository_root,
        "linux-x86_64".to_owned(),
        "x86_64-v1".to_owned(),
        "test-runtime".to_owned(),
        allowed.clone(),
        128 * 1024 * 1024,
    )
    .unwrap();
    let installed = repository.install(&metadata, &library, &allowed).unwrap();
    assert_eq!(installed.sdk_release, 1);
    assert_eq!(installed.native_abis, vec![1, 2, 3, 4, 5]);
    assert_eq!(repository.list().unwrap(), vec![installed]);

    let reopened_while_installed = SdkReleaseRegistry::open(
        &repository_root,
        "linux-x86_64".to_owned(),
        "x86_64-v1".to_owned(),
        "test-runtime".to_owned(),
        allowed.clone(),
        128 * 1024 * 1024,
    )
    .unwrap();
    assert_eq!(reopened_while_installed.list().unwrap().len(), 1);
    drop(reopened_while_installed);

    let pin = repository.resolve(&manifest(4, 1)).unwrap();
    assert_eq!(
        repository.remove_release(1).unwrap_err(),
        SdkRegistryError::ReleasePinned
    );
    let installed_path = pin.library_path().to_path_buf();
    drop(pin);
    assert_eq!(repository.remove_release(1).unwrap(), library.len() as u64);
    assert!(!installed_path.exists());
    assert!(repository.list().unwrap().is_empty());

    let reopened = SdkReleaseRegistry::open(
        &repository_root,
        "linux-x86_64".to_owned(),
        "x86_64-v1".to_owned(),
        "test-runtime".to_owned(),
        allowed,
        128 * 1024 * 1024,
    )
    .unwrap();
    assert!(reopened.list().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repository_streamed_install_is_contiguous_owner_bound_and_durable() {
    let root = temp_dir();
    fs::create_dir(&root).unwrap();
    let release = built_release(&root);
    let metadata = fs::read(release.join("sdk-release.json")).unwrap();
    let library = fs::read(release.join("libprdb_sql.so.1")).unwrap();
    let repository_root = root.join("instance").join("sdk-libraries");
    let allowed = HashSet::from(["libc.so.6".to_owned()]);
    let repository = SdkReleaseRegistry::open(
        &repository_root,
        "linux-x86_64".to_owned(),
        "x86_64-v1".to_owned(),
        "test-runtime".to_owned(),
        allowed,
        128 * 1024 * 1024,
    )
    .unwrap();
    let digest: [u8; 32] = Sha256::digest(&library).into();
    let operation_id = [0x91; 16];
    let owner_id = [0x72; 16];
    assert_eq!(
        repository
            .begin_install_upload(
                operation_id,
                owner_id,
                metadata.len() as u64,
                library.len() as u64,
                digest,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        repository
            .upload_install_chunk(operation_id, [0x73; 16], 0, &metadata[..1])
            .unwrap_err(),
        SdkRegistryError::UploadOwnerMismatch
    );
    assert_eq!(
        repository
            .upload_install_chunk(operation_id, owner_id, 1, &metadata[..1])
            .unwrap_err(),
        SdkRegistryError::InvalidMetadata
    );
    let mut offset = 0u64;
    for bytes in [&metadata[..], &library[..]] {
        for chunk in bytes.chunks(32 * 1024) {
            offset = repository
                .upload_install_chunk(operation_id, owner_id, offset, chunk)
                .unwrap();
        }
    }
    let installed = repository
        .finish_install_upload(operation_id, owner_id)
        .unwrap();
    assert_eq!(installed.sdk_release, 1);
    assert_eq!(installed.sha256, digest);
    assert_eq!(repository.list().unwrap(), vec![installed.clone()]);
    assert_eq!(
        repository
            .finish_install_upload(operation_id, owner_id)
            .unwrap(),
        installed
    );
    drop(repository);
    let reopened = SdkReleaseRegistry::open(
        &repository_root,
        "linux-x86_64".to_owned(),
        "x86_64-v1".to_owned(),
        "test-runtime".to_owned(),
        HashSet::from(["libc.so.6".to_owned()]),
        128 * 1024 * 1024,
    )
    .unwrap();
    assert_eq!(reopened.list().unwrap(), vec![installed]);
    assert!(matches!(
        reopened.operation_outcome(operation_id).unwrap(),
        Some(prdb_engine::sdk_registry::SdkLibraryOperationOutcome::Installed {
            owner_id: recorded_owner,
            sdk_release: 1,
            library_bytes,
            ..
        }) if recorded_owner == owner_id && library_bytes > 0
    ));
    drop(reopened);
    fs::set_permissions(
        repository_root.join("release-1"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repository_rejects_identity_replacement_digest_corruption_and_disallowed_dependencies() {
    let root = temp_dir();
    fs::create_dir(&root).unwrap();
    let release = built_release(&root);
    let metadata = fs::read(release.join("sdk-release.json")).unwrap();
    let library = fs::read(release.join("libprdb_sql.so.1")).unwrap();
    let repository_root = root.join("repository");
    let allowed = HashSet::from(["libc.so.6".to_owned()]);
    let repository = SdkReleaseRegistry::open(
        &repository_root,
        "linux-x86_64".to_owned(),
        "x86_64-v1".to_owned(),
        "test-runtime".to_owned(),
        allowed.clone(),
        128 * 1024 * 1024,
    )
    .unwrap();
    repository.install(&metadata, &library, &allowed).unwrap();
    assert_eq!(
        repository.install(&metadata, &library, &allowed).unwrap(),
        repository.list().unwrap()[0]
    );

    let mut changed_library = library.clone();
    changed_library.push(0);
    assert_eq!(
        repository
            .install(&metadata, &changed_library, &allowed)
            .unwrap_err(),
        SdkRegistryError::DigestMismatch
    );
    let changed_digest = Sha256::digest(&changed_library)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let original_digest = Sha256::digest(&library)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let replacement_metadata = String::from_utf8(metadata.clone())
        .unwrap()
        .replace(&original_digest, &changed_digest);
    assert_eq!(
        repository
            .install(replacement_metadata.as_bytes(), &changed_library, &allowed)
            .unwrap_err(),
        SdkRegistryError::ReleaseConflict
    );
    let mut wrong_dependencies = allowed.clone();
    wrong_dependencies.clear();
    assert_eq!(
        repository
            .install(&metadata, &library, &wrong_dependencies)
            .unwrap_err(),
        SdkRegistryError::DependencyNotAllowed
    );

    let mut changed_metadata = String::from_utf8(metadata).unwrap();
    changed_metadata = changed_metadata.replace("x86_64-v1", "x86_64-v2");
    assert_eq!(
        repository
            .install(changed_metadata.as_bytes(), &library, &allowed)
            .unwrap_err(),
        SdkRegistryError::IncompatibleProfile
    );
    fs::set_permissions(
        repository_root.join("release-1"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repository_open_discards_only_incomplete_install_staging_directories() {
    let root = temp_dir();
    fs::create_dir_all(root.join("repository/.install-interrupted")).unwrap();
    fs::write(
        root.join("repository/.install-interrupted/partial"),
        b"partial",
    )
    .unwrap();
    let repository = SdkReleaseRegistry::open(
        &root.join("repository"),
        "linux-x86_64".to_owned(),
        "x86_64-v1".to_owned(),
        "test-runtime".to_owned(),
        HashSet::from(["libc.so.6".to_owned()]),
        128 * 1024 * 1024,
    )
    .unwrap();
    assert!(repository.list().unwrap().is_empty());
    assert_eq!(fs::read_dir(root.join("repository")).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}
