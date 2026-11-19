//! Durable administrator-managed repository for immutable SQL SDK releases.

use crate::submission_manifest::{ArtifactManifestV1, decode_elf_dynamic_info};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

const MAX_RELEASE_LIBRARY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RELEASE_METADATA_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReleaseRecord {
    sdk_release: u32,
    soname: String,
    sha256: [u8; 32],
    cpu_baseline: String,
    runtime_profile: String,
    native_abis: Vec<u8>,
    needed: Vec<String>,
}

#[derive(Debug)]
struct CachedRelease {
    record: ReleaseRecord,
    library_path: PathBuf,
    library_bytes: u64,
    pins: AtomicUsize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SdkReleaseInfo {
    pub sdk_release: u32,
    pub native_abis: Vec<u8>,
    pub target: String,
    pub cpu_baseline: String,
    pub runtime_profile: String,
    pub soname: String,
    pub sha256: [u8; 32],
    pub needed: Vec<String>,
    pub library_bytes: u64,
}

/// A strong reference keeps the immutable repository release from removal.
#[derive(Debug)]
pub struct SdkLibraryPin(Arc<CachedRelease>);

impl Clone for SdkLibraryPin {
    fn clone(&self) -> Self {
        self.0.pins.fetch_add(1, Ordering::AcqRel);
        Self(Arc::clone(&self.0))
    }
}

impl Drop for SdkLibraryPin {
    fn drop(&mut self) {
        self.0.pins.fetch_sub(1, Ordering::AcqRel);
    }
}

impl SdkLibraryPin {
    pub fn library_path(&self) -> &Path {
        &self.0.library_path
    }

    pub fn soname(&self) -> &str {
        &self.0.record.soname
    }

    pub fn sha256(&self) -> [u8; 32] {
        self.0.record.sha256
    }
}

/// A startup-validated registry for one target and runtime compatibility
/// profile. Lookups are read-only and return pins that protect files in use.
#[derive(Debug)]
pub struct SdkReleaseRegistry {
    cache_root: PathBuf,
    target: String,
    cpu_baseline: String,
    runtime_profile: String,
    max_cache_bytes: u64,
    releases: RwLock<HashMap<(u32, u8), Arc<CachedRelease>>>,
}

/// Repository-oriented name for the release manager. The older name remains
/// temporarily available to the legacy deployment-cache path during migration.
pub type SdkLibraryRepository = SdkReleaseRegistry;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SdkRegistryError {
    InvalidConfiguration,
    InvalidMetadata,
    IncompatibleProfile,
    UnavailableRelease,
    DigestMismatch,
    CacheFull,
    AtomicPublishUnsupported,
    ReleaseConflict,
    ReleasePinned,
    DependencyNotAllowed,
    Io(io::ErrorKind),
}

impl From<io::Error> for SdkRegistryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

impl SdkReleaseRegistry {
    /// Open an instance-local durable repository and validate every published
    /// release before making it available for resolution.
    pub fn open(
        repository_root: &Path,
        target: String,
        cpu_baseline: String,
        runtime_profile: String,
        allowed_dependencies: HashSet<String>,
        max_repository_bytes: u64,
    ) -> Result<Self, SdkRegistryError> {
        if max_repository_bytes == 0
            || target != "linux-x86_64"
            || !is_profile_token(&cpu_baseline)
            || !is_profile_token(&runtime_profile)
        {
            return Err(SdkRegistryError::InvalidConfiguration);
        }
        let repository_root = prepare_repository_root(repository_root)?;
        let mut releases = HashMap::new();
        let mut repository_bytes = 0u64;
        for item in fs::read_dir(&repository_root)? {
            let item = item?;
            let name = item.file_name();
            let name = name.to_str().ok_or(SdkRegistryError::InvalidMetadata)?;
            let path = item.path();
            let file_type = item.file_type()?;
            if name.starts_with(".install-") {
                if !file_type.is_dir() {
                    return Err(SdkRegistryError::InvalidMetadata);
                }
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                fs::remove_dir_all(path)?;
                continue;
            }
            if !name.starts_with("release-") || !file_type.is_dir() {
                return Err(SdkRegistryError::InvalidMetadata);
            }
            let directory_release = name["release-".len()..]
                .parse::<u32>()
                .map_err(|_| SdkRegistryError::InvalidMetadata)?;
            let children = fs::read_dir(&path)?
                .map(|child| {
                    child
                        .map(|child| child.file_name())
                        .map_err(SdkRegistryError::from)
                })
                .collect::<Result<HashSet<_>, _>>()?;
            let expected_children = HashSet::from([
                std::ffi::OsString::from("sdk-release.json"),
                std::ffi::OsString::from(format!("libprdb_sql.so.{directory_release}")),
            ]);
            if children != expected_children {
                return Err(SdkRegistryError::InvalidMetadata);
            }
            validate_immutable_permissions(&path)?;
            let metadata =
                read_regular(&path.join("sdk-release.json"), MAX_RELEASE_METADATA_BYTES)?;
            let record = parse_release_record(&metadata)?;
            if directory_release != record.sdk_release
                || record.cpu_baseline != cpu_baseline
                || record.runtime_profile != runtime_profile
                || record
                    .needed
                    .iter()
                    .any(|dependency| !allowed_dependencies.contains(dependency))
            {
                return Err(SdkRegistryError::IncompatibleProfile);
            }
            let library_path = path.join(&record.soname);
            validate_immutable_permissions(&path.join("sdk-release.json"))?;
            validate_immutable_permissions(&library_path)?;
            let library = read_regular(&library_path, MAX_RELEASE_LIBRARY_BYTES)?;
            validate_library(&record, &library)?;
            if sha256(&library) != record.sha256 {
                return Err(SdkRegistryError::DigestMismatch);
            }
            repository_bytes = repository_bytes
                .checked_add(library.len() as u64)
                .ok_or(SdkRegistryError::CacheFull)?;
            if repository_bytes > max_repository_bytes {
                return Err(SdkRegistryError::CacheFull);
            }
            let entry = Arc::new(CachedRelease {
                record: record.clone(),
                library_path,
                library_bytes: library.len() as u64,
                pins: AtomicUsize::new(0),
            });
            insert_release_abis(&mut releases, entry)?;
        }
        File::open(&repository_root)?.sync_all()?;
        Ok(Self {
            cache_root: repository_root,
            target,
            cpu_baseline,
            runtime_profile,
            max_cache_bytes: max_repository_bytes,
            releases: RwLock::new(releases),
        })
    }

    /// Validate installed release directories and atomically copy their
    /// libraries into a digest-keyed, read-only cache. Each directory must
    /// contain `sdk-release.json` and the named versioned library.
    pub fn load(
        release_directories: &[PathBuf],
        cache_root: &Path,
        target: String,
        cpu_baseline: String,
        runtime_profile: String,
        max_cache_bytes: u64,
    ) -> Result<Self, SdkRegistryError> {
        if release_directories.is_empty()
            || max_cache_bytes == 0
            || !is_profile_token(&target)
            || !is_profile_token(&cpu_baseline)
            || !is_profile_token(&runtime_profile)
        {
            return Err(SdkRegistryError::InvalidConfiguration);
        }
        let cache_root = prepare_cache_root(cache_root)?;
        let mut releases = HashMap::new();
        let mut cached_bytes = 0u64;
        let mut cached_digests = HashMap::<[u8; 32], u64>::new();
        for directory in release_directories {
            let directory = fs::canonicalize(directory)?;
            if !directory.is_dir() {
                return Err(SdkRegistryError::InvalidConfiguration);
            }
            let metadata_path = directory.join("sdk-release.json");
            let metadata = read_regular(&metadata_path, MAX_RELEASE_METADATA_BYTES)?;
            let record = parse_release_record(&metadata)?;
            if record.cpu_baseline != cpu_baseline || record.runtime_profile != runtime_profile {
                return Err(SdkRegistryError::IncompatibleProfile);
            }
            if target != "linux-x86_64" {
                return Err(SdkRegistryError::IncompatibleProfile);
            }
            let library_path = directory.join(format!("libprdb_sql.so.{}", record.sdk_release));
            let library = read_regular(&library_path, MAX_RELEASE_LIBRARY_BYTES)?;
            validate_library(&record, &library)?;
            if sha256(&library) != record.sha256 {
                return Err(SdkRegistryError::DigestMismatch);
            }
            if let std::collections::hash_map::Entry::Vacant(entry) =
                cached_digests.entry(record.sha256)
            {
                cached_bytes = cached_bytes
                    .checked_add(library.len() as u64)
                    .ok_or(SdkRegistryError::CacheFull)?;
                if cached_bytes > max_cache_bytes {
                    return Err(SdkRegistryError::CacheFull);
                }
                entry.insert(library.len() as u64);
            }
            let cached_path = publish_cache_entry(&cache_root, &library, &record)?;
            let entry = Arc::new(CachedRelease {
                record: record.clone(),
                library_path: cached_path,
                library_bytes: library.len() as u64,
                pins: AtomicUsize::new(0),
            });
            for abi in &record.native_abis {
                if releases
                    .insert((record.sdk_release, *abi), Arc::clone(&entry))
                    .is_some()
                {
                    return Err(SdkRegistryError::InvalidMetadata);
                }
            }
        }
        Ok(Self {
            cache_root,
            target,
            cpu_baseline,
            runtime_profile,
            max_cache_bytes,
            releases: RwLock::new(releases),
        })
    }

    /// Resolve and pin the release described by a validated PMF1 manifest.
    /// The artifact dependency list must include this exact SONAME and all
    /// dependencies recorded for the SDK shared object.
    pub fn resolve(
        &self,
        manifest: &ArtifactManifestV1,
    ) -> Result<SdkLibraryPin, SdkRegistryError> {
        if manifest.cpu_baseline != self.cpu_baseline
            || manifest.runtime_profile != self.runtime_profile
        {
            return Err(SdkRegistryError::IncompatibleProfile);
        }
        let releases = self
            .releases
            .read()
            .map_err(|_| SdkRegistryError::InvalidConfiguration)?;
        let release = releases
            .get(&(manifest.sdk_release, manifest.native_abi_version))
            .ok_or(SdkRegistryError::UnavailableRelease)?;
        if self.target != "linux-x86_64"
            || !manifest
                .dependencies
                .iter()
                .any(|name| name == &release.record.soname)
            || release
                .record
                .needed
                .iter()
                .any(|dependency| !manifest.dependencies.contains(dependency))
        {
            return Err(SdkRegistryError::IncompatibleProfile);
        }
        let library = read_regular(&release.library_path, MAX_RELEASE_LIBRARY_BYTES)?;
        if sha256(&library) != release.record.sha256 {
            return Err(SdkRegistryError::DigestMismatch);
        }
        validate_library(&release.record, &library)?;
        release.pins.fetch_add(1, Ordering::AcqRel);
        Ok(SdkLibraryPin(Arc::clone(release)))
    }

    /// Validate and atomically install one immutable release. Metadata and
    /// library bytes are supplied by the authenticated repository API.
    pub fn install(
        &self,
        metadata: &[u8],
        library: &[u8],
        allowed_dependencies: &HashSet<String>,
    ) -> Result<SdkReleaseInfo, SdkRegistryError> {
        if library.is_empty()
            || library.len() as u64 > MAX_RELEASE_LIBRARY_BYTES
            || metadata.is_empty()
            || metadata.len() as u64 > MAX_RELEASE_METADATA_BYTES
        {
            return Err(SdkRegistryError::InvalidMetadata);
        }
        let record = parse_release_record(metadata)?;
        if record.cpu_baseline != self.cpu_baseline
            || record.runtime_profile != self.runtime_profile
            || self.target != "linux-x86_64"
        {
            return Err(SdkRegistryError::IncompatibleProfile);
        }
        if record
            .needed
            .iter()
            .any(|dependency| !allowed_dependencies.contains(dependency))
        {
            return Err(SdkRegistryError::DependencyNotAllowed);
        }
        if sha256(library) != record.sha256 {
            return Err(SdkRegistryError::DigestMismatch);
        }
        validate_library(&record, library)?;

        let mut releases = self
            .releases
            .write()
            .map_err(|_| SdkRegistryError::InvalidConfiguration)?;
        if let Some(existing) = releases
            .values()
            .find(|entry| entry.record.sdk_release == record.sdk_release)
        {
            if existing.record == record && existing.library_bytes == library.len() as u64 {
                let cached = read_regular(&existing.library_path, MAX_RELEASE_LIBRARY_BYTES)?;
                if sha256(&cached) != record.sha256 {
                    return Err(SdkRegistryError::DigestMismatch);
                }
                return Ok(release_info(&record, library.len() as u64));
            }
            return Err(SdkRegistryError::ReleaseConflict);
        }
        let mut unique_releases = HashMap::<u32, u64>::new();
        for entry in releases.values() {
            unique_releases
                .entry(entry.record.sdk_release)
                .or_insert(entry.library_bytes);
        }
        let current_bytes = unique_releases
            .values()
            .try_fold(0u64, |total, bytes| total.checked_add(*bytes))
            .ok_or(SdkRegistryError::CacheFull)?;
        let next_bytes = current_bytes
            .checked_add(library.len() as u64)
            .ok_or(SdkRegistryError::CacheFull)?;
        if next_bytes > self.max_cache_bytes {
            return Err(SdkRegistryError::CacheFull);
        }

        let destination = self
            .cache_root
            .join(format!("release-{}", record.sdk_release));
        let published =
            publish_repository_entry(&self.cache_root, &destination, metadata, library, &record)?;
        let entry = Arc::new(CachedRelease {
            record: record.clone(),
            library_path: published,
            library_bytes: library.len() as u64,
            pins: AtomicUsize::new(0),
        });
        insert_release_abis(&mut releases, entry)?;
        Ok(release_info(&record, library.len() as u64))
    }

    /// Return bounded compatibility metadata in stable release/ABI order.
    pub fn list(&self) -> Result<Vec<SdkReleaseInfo>, SdkRegistryError> {
        let releases = self
            .releases
            .read()
            .map_err(|_| SdkRegistryError::InvalidConfiguration)?;
        let mut unique = HashMap::<u32, SdkReleaseInfo>::new();
        for entry in releases.values() {
            unique
                .entry(entry.record.sdk_release)
                .or_insert_with(|| release_info(&entry.record, entry.library_bytes));
        }
        let mut result = unique.into_values().collect::<Vec<_>>();
        result.sort_by_key(|entry| entry.sdk_release);
        Ok(result)
    }

    /// Remove a complete SDK release after all ABI pins have drained.
    pub fn remove_release(&self, sdk_release: u32) -> Result<u64, SdkRegistryError> {
        let mut releases = self
            .releases
            .write()
            .map_err(|_| SdkRegistryError::InvalidConfiguration)?;
        let entry = releases
            .values()
            .find(|entry| entry.record.sdk_release == sdk_release)
            .cloned()
            .ok_or(SdkRegistryError::UnavailableRelease)?;
        if entry.pins.load(Ordering::Acquire) != 0 {
            return Err(SdkRegistryError::ReleasePinned);
        }
        let directory = entry
            .library_path
            .parent()
            .ok_or(SdkRegistryError::InvalidConfiguration)?;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        fs::remove_dir_all(directory)?;
        releases.retain(|(release, _), _| *release != sdk_release);
        File::open(&self.cache_root)?.sync_all()?;
        Ok(entry.library_bytes)
    }

    pub fn cache_bytes(&self) -> Result<u64, SdkRegistryError> {
        let releases = self
            .releases
            .read()
            .map_err(|_| SdkRegistryError::InvalidConfiguration)?;
        let mut unique = HashMap::<[u8; 32], u64>::new();
        for entry in releases.values() {
            unique
                .entry(entry.record.sha256)
                .or_insert(entry.library_bytes);
        }
        unique
            .values()
            .try_fold(0u64, |total, bytes| total.checked_add(*bytes))
            .filter(|total| *total <= self.max_cache_bytes)
            .ok_or(SdkRegistryError::CacheFull)
    }

    /// Remove only cache entries that have no outstanding invocation pins.
    pub fn evict_unpinned(&self) -> Result<u64, SdkRegistryError> {
        let mut releases = self
            .releases
            .write()
            .map_err(|_| SdkRegistryError::InvalidConfiguration)?;
        let mut evicted = HashMap::<[u8; 32], (PathBuf, u64)>::new();
        for entry in releases.values() {
            if entry.pins.load(Ordering::Acquire) == 0 {
                evicted.entry(entry.record.sha256).or_insert_with(|| {
                    (
                        entry.library_path.parent().unwrap().to_path_buf(),
                        entry.library_bytes,
                    )
                });
            }
        }
        releases.retain(|_, entry| !evicted.contains_key(&entry.record.sha256));
        let mut reclaimed = 0u64;
        for (_, (directory, bytes)) in evicted {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
            fs::remove_dir_all(directory)?;
            reclaimed = reclaimed.saturating_add(bytes);
        }
        Ok(reclaimed)
    }

    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    pub fn sonames(&self) -> Result<Vec<String>, SdkRegistryError> {
        let releases = self
            .releases
            .read()
            .map_err(|_| SdkRegistryError::InvalidConfiguration)?;
        let mut names = releases
            .values()
            .map(|entry| entry.record.soname.clone())
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Ok(names)
    }
}

fn prepare_cache_root(path: &Path) -> Result<PathBuf, SdkRegistryError> {
    fs::create_dir_all(path)?;
    let path = fs::canonicalize(path)?;
    if !path.is_dir() {
        return Err(SdkRegistryError::InvalidConfiguration);
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

fn prepare_repository_root(path: &Path) -> Result<PathBuf, SdkRegistryError> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_dir() {
            return Err(SdkRegistryError::InvalidConfiguration);
        }
    } else {
        fs::create_dir_all(path)?;
    }
    let path = fs::canonicalize(path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

fn validate_immutable_permissions(path: &Path) -> Result<(), SdkRegistryError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.permissions().mode() & 0o222 != 0 {
        return Err(SdkRegistryError::InvalidMetadata);
    }
    Ok(())
}

fn insert_release_abis(
    releases: &mut HashMap<(u32, u8), Arc<CachedRelease>>,
    entry: Arc<CachedRelease>,
) -> Result<(), SdkRegistryError> {
    if releases
        .keys()
        .any(|(release, _)| *release == entry.record.sdk_release)
    {
        return Err(SdkRegistryError::InvalidMetadata);
    }
    for abi in &entry.record.native_abis {
        releases.insert((entry.record.sdk_release, *abi), Arc::clone(&entry));
    }
    Ok(())
}

fn release_info(record: &ReleaseRecord, library_bytes: u64) -> SdkReleaseInfo {
    SdkReleaseInfo {
        sdk_release: record.sdk_release,
        native_abis: record.native_abis.clone(),
        target: "linux-x86_64".to_owned(),
        cpu_baseline: record.cpu_baseline.clone(),
        runtime_profile: record.runtime_profile.clone(),
        soname: record.soname.clone(),
        sha256: record.sha256,
        needed: record.needed.clone(),
        library_bytes,
    }
}

fn publish_repository_entry(
    repository_root: &Path,
    destination: &Path,
    metadata: &[u8],
    library: &[u8],
    record: &ReleaseRecord,
) -> Result<PathBuf, SdkRegistryError> {
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| SdkRegistryError::InvalidConfiguration)?;
    let temporary = repository_root.join(format!(
        ".install-{}-{}",
        std::process::id(),
        lower_hex(&nonce)
    ));
    fs::create_dir(&temporary)?;
    let staged_library = temporary.join(&record.soname);
    let staged_metadata = temporary.join("sdk-release.json");
    let result = (|| {
        write_new_synced(&staged_metadata, metadata)?;
        write_new_synced(&staged_library, library)?;
        if sha256(&read_regular(&staged_library, MAX_RELEASE_LIBRARY_BYTES)?) != record.sha256 {
            return Err(SdkRegistryError::DigestMismatch);
        }
        validate_library(
            record,
            &read_regular(&staged_library, MAX_RELEASE_LIBRARY_BYTES)?,
        )?;
        File::open(&temporary)?.sync_all()?;
        fs::set_permissions(&staged_metadata, fs::Permissions::from_mode(0o444))?;
        fs::set_permissions(&staged_library, fs::Permissions::from_mode(0o444))?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o500))?;
        File::open(&temporary)?.sync_all()?;
        rename_noreplace(&temporary, destination)?;
        File::open(repository_root)?.sync_all()?;
        Ok(destination.join(&record.soname))
    })();
    if result.is_err() {
        let _ = fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), SdkRegistryError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, SdkRegistryError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() > limit {
        return Err(SdkRegistryError::InvalidMetadata);
    }
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(SdkRegistryError::InvalidMetadata);
    }
    Ok(bytes)
}

fn parse_release_record(bytes: &[u8]) -> Result<ReleaseRecord, SdkRegistryError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SdkRegistryError::InvalidMetadata)?;
    if !text.is_ascii() || !text.ends_with('\n') {
        return Err(SdkRegistryError::InvalidMetadata);
    }
    let sdk_release = json_integer(text, "sdk_release")?;
    let format = json_string(text, "format")?;
    let library_name = json_string(text, "library")?;
    let target = json_string(text, "target")?;
    let digest_text = json_string(text, "sha256")?;
    let record = ReleaseRecord {
        sdk_release,
        soname: json_string(text, "soname")?,
        sha256: parse_hex_digest(&digest_text)?,
        cpu_baseline: json_string(text, "cpu_baseline")?,
        runtime_profile: json_string(text, "runtime_profile")?,
        native_abis: json_integer_array(text, "native_abis")?
            .into_iter()
            .map(|abi| u8::try_from(abi).map_err(|_| SdkRegistryError::InvalidMetadata))
            .collect::<Result<_, _>>()?,
        needed: json_string_array(text, "needed")?,
    };
    let canonical = format!(
        "{{\"cpu_baseline\":\"{}\",\"format\":\"{}\",\"library\":\"{}\",\"native_abis\":[{}],\"needed\":[{}],\"runtime_profile\":\"{}\",\"sdk_release\":{},\"sha256\":\"{}\",\"soname\":\"{}\",\"target\":\"{}\"}}\n",
        record.cpu_baseline,
        format,
        library_name,
        record
            .native_abis
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(","),
        record
            .needed
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(","),
        record.runtime_profile,
        record.sdk_release,
        digest_text,
        record.soname,
        target,
    );
    if canonical != text
        || format != "PRDB-SDK-RELEASE-1"
        || target != "linux-x86_64"
        || sdk_release == 0
        || record.soname != format!("libprdb_sql.so.{sdk_release}")
        || library_name != record.soname
        || record.native_abis.is_empty()
        || record.native_abis.iter().any(|abi| !(1..=5).contains(abi))
        || record.native_abis.windows(2).any(|pair| pair[0] >= pair[1])
        || record.needed.windows(2).any(|pair| pair[0] >= pair[1])
        || record
            .needed
            .iter()
            .any(|dependency| dependency.contains('/') || dependency.contains('\\'))
        || !is_profile_token(&record.cpu_baseline)
        || !is_profile_token(&record.runtime_profile)
    {
        return Err(SdkRegistryError::InvalidMetadata);
    }
    Ok(record)
}

fn validate_library(record: &ReleaseRecord, library: &[u8]) -> Result<(), SdkRegistryError> {
    let dynamic =
        decode_elf_dynamic_info(library).map_err(|_| SdkRegistryError::InvalidMetadata)?;
    if dynamic.soname.as_deref() != Some(record.soname.as_str())
        || dynamic.rpath.is_some()
        || dynamic.runpath.is_some()
        || dynamic.needed != record.needed
    {
        return Err(SdkRegistryError::InvalidMetadata);
    }
    Ok(())
}

fn publish_cache_entry(
    cache_root: &Path,
    library: &[u8],
    record: &ReleaseRecord,
) -> Result<PathBuf, SdkRegistryError> {
    let digest = lower_hex(&record.sha256);
    let abi_key = record
        .native_abis
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join("-");
    let identity = format!(
        "{digest}-linux-x86_64-{}-{}-{}-{abi_key}",
        record.cpu_baseline, record.runtime_profile, record.sdk_release
    );
    let bucket = cache_root.join(identity);
    let cached_library = bucket.join(&record.soname);
    if bucket.exists() {
        let cached = read_regular(&cached_library, MAX_RELEASE_LIBRARY_BYTES)?;
        if sha256(&cached) != record.sha256 {
            return Err(SdkRegistryError::DigestMismatch);
        }
        validate_library(record, &cached)?;
        return Ok(cached_library);
    }
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| SdkRegistryError::InvalidConfiguration)?;
    let nonce = lower_hex(&nonce);
    let temporary = cache_root.join(format!(".release-{}-{nonce}-{digest}", std::process::id()));
    match fs::create_dir(&temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(SdkRegistryError::InvalidConfiguration);
        }
        Err(error) => return Err(error.into()),
    }
    let staged = temporary.join(&record.soname);
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged)?;
        output.write_all(library)?;
        output.sync_all()?;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o444))?;
        if sha256(&read_regular(&staged, MAX_RELEASE_LIBRARY_BYTES)?) != record.sha256 {
            return Err(SdkRegistryError::DigestMismatch);
        }
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o500))?;
        rename_noreplace(&temporary, &bucket)?;
        File::open(cache_root)?.sync_all()?;
        Ok(cached_library.clone())
    })();
    match result {
        Ok(path) => Ok(path),
        Err(error) => {
            let _ = fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700));
            let _ = fs::remove_dir_all(&temporary);
            if error == SdkRegistryError::Io(io::ErrorKind::AlreadyExists) {
                let cached = read_regular(&cached_library, MAX_RELEASE_LIBRARY_BYTES)?;
                if sha256(&cached) != record.sha256 {
                    return Err(SdkRegistryError::DigestMismatch);
                }
                validate_library(record, &cached)?;
                return Ok(cached_library);
            }
            Err(error)
        }
    }
}

fn rename_noreplace(source: &Path, destination: &Path) -> Result<(), SdkRegistryError> {
    use std::os::unix::ffi::OsStrExt;
    let source = std::ffi::CString::new(source.as_os_str().as_bytes())
        .map_err(|_| SdkRegistryError::InvalidConfiguration)?;
    let destination = std::ffi::CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| SdkRegistryError::InvalidConfiguration)?;
    // SAFETY: both absolute C strings remain live through the syscall.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINVAL) {
            Err(SdkRegistryError::AtomicPublishUnsupported)
        } else {
            Err(error.into())
        }
    }
}

fn json_value<'a>(text: &'a str, key: &str) -> Result<&'a str, SdkRegistryError> {
    let needle = format!("\"{key}\":");
    if text.matches(&needle).count() != 1 {
        return Err(SdkRegistryError::InvalidMetadata);
    }
    let start = text
        .find(&needle)
        .ok_or(SdkRegistryError::InvalidMetadata)?
        + needle.len();
    let rest = &text[start..];
    let end = if let Some(content) = rest.strip_prefix('"') {
        let length = content.find('"').ok_or(SdkRegistryError::InvalidMetadata)?;
        if content[..length].contains('\\') {
            return Err(SdkRegistryError::InvalidMetadata);
        }
        length + 2
    } else if rest.starts_with('[') {
        rest.find(']').ok_or(SdkRegistryError::InvalidMetadata)? + 1
    } else {
        rest.find([',', '}'])
            .ok_or(SdkRegistryError::InvalidMetadata)?
    };
    Ok(&rest[..end])
}

fn json_string(text: &str, key: &str) -> Result<String, SdkRegistryError> {
    json_value(text, key)?
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_owned)
        .ok_or(SdkRegistryError::InvalidMetadata)
}

fn json_integer(text: &str, key: &str) -> Result<u32, SdkRegistryError> {
    json_value(text, key)?
        .parse()
        .map_err(|_| SdkRegistryError::InvalidMetadata)
}

fn json_integer_array(text: &str, key: &str) -> Result<Vec<u64>, SdkRegistryError> {
    parse_array(json_value(text, key)?)
        .into_iter()
        .map(|item| item.parse().map_err(|_| SdkRegistryError::InvalidMetadata))
        .collect()
}

fn json_string_array(text: &str, key: &str) -> Result<Vec<String>, SdkRegistryError> {
    parse_array(json_value(text, key)?)
        .into_iter()
        .map(|item| {
            item.strip_prefix('"')
                .and_then(|item| item.strip_suffix('"'))
                .filter(|item| !item.contains('\\'))
                .map(str::to_owned)
                .ok_or(SdkRegistryError::InvalidMetadata)
        })
        .collect()
}

fn parse_array(value: &str) -> Vec<&str> {
    let inner = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or("");
    if inner.is_empty() {
        Vec::new()
    } else {
        inner.split(',').collect()
    }
}

fn parse_hex_digest(value: &str) -> Result<[u8; 32], SdkRegistryError> {
    if value.len() != 64 {
        return Err(SdkRegistryError::InvalidMetadata);
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| SdkRegistryError::InvalidMetadata)?;
        if value.as_bytes()[index * 2].is_ascii_uppercase()
            || value.as_bytes()[index * 2 + 1].is_ascii_uppercase()
        {
            return Err(SdkRegistryError::InvalidMetadata);
        }
    }
    Ok(digest)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn is_profile_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}
