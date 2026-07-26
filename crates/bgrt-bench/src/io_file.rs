//! Scratch file plus cache-bypassing random reads.
//!
//! A disk benchmark that reads through the page cache measures `memcpy`, not the
//! device — and a `QosClass`'s I/O priority only ever applies to requests that
//! actually reach the block layer. So every read here is issued on a handle
//! opened to bypass the cache: `O_DIRECT` on Linux, `F_NOCACHE` on macOS,
//! `FILE_FLAG_NO_BUFFERING` on Windows.
//!
//! Bypass can fail (tmpfs and some filesystems reject `O_DIRECT`). That is
//! reported as [`CacheBypass::Buffered`] and surfaced in the output rather than
//! silently producing page-cache numbers dressed up as disk numbers.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Alignment (bytes) required by direct/unbuffered I/O: buffer address, file
/// offset, and transfer length must all be multiples of this.
pub const ALIGN: usize = 4096;

/// Chunk size used when laying down the scratch file.
const FILL_CHUNK: usize = 4 * 1024 * 1024;

/// Whether reads actually reach the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheBypass {
    /// Cache bypass is in effect; reads reach the block layer.
    Direct,
    /// Bypass unavailable — reads may be served from the page cache.
    Buffered,
}

/// Assume the worst until a handle proves otherwise.
impl Default for CacheBypass {
    fn default() -> Self {
        CacheBypass::Buffered
    }
}

impl CacheBypass {
    /// Stable lowercase label for the table/JSON output.
    pub fn label(self) -> &'static str {
        match self {
            CacheBypass::Direct => "direct",
            CacheBypass::Buffered => "buffered",
        }
    }

    /// Combine two observations: any buffered handle taints the phase.
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (CacheBypass::Direct, CacheBypass::Direct) => CacheBypass::Direct,
            _ => CacheBypass::Buffered,
        }
    }
}

/// Round `bytes` down to a usable direct-I/O block size.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidInput`] if the size is zero or not a multiple
/// of [`ALIGN`].
pub fn validate_block(bytes: usize) -> io::Result<usize> {
    if bytes == 0 || bytes % ALIGN != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("block size must be a non-zero multiple of {ALIGN} bytes, got {bytes}"),
        ));
    }
    Ok(bytes)
}

/// Number of whole `block`-sized slots in a file of `size` bytes (never zero, so
/// callers can take a modulus).
pub fn blocks_in(size: u64, block: usize) -> u64 {
    (size / block as u64).max(1)
}

/// Map a random number onto a block-aligned offset within the file.
pub fn block_offset(rand: u64, blocks: u64, block: usize) -> u64 {
    (rand % blocks.max(1)) * block as u64
}

/// A scratch file to read from, removed on drop unless `keep` was set.
pub struct ScratchFile {
    path: PathBuf,
    size: u64,
    keep: bool,
}

impl ScratchFile {
    /// Create (or reuse) a scratch file of `size` bytes in `dir`.
    ///
    /// An existing file of exactly the right size is reused — laying down
    /// hundreds of MiB on every run is slow and wears the device for nothing.
    /// Content is pseudo-random so a compressing filesystem can't serve the
    /// reads from thin air.
    ///
    /// # Errors
    ///
    /// Returns any I/O error from creating, writing, or syncing the file.
    pub fn create(dir: &Path, size: u64, keep: bool) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("bgrt-bench-io.dat");
        let reusable = std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == size);
        if !reusable {
            fill(&path, size)?;
        }
        let scratch = Self { path, size, keep };
        scratch.drop_cache();
        Ok(scratch)
    }

    /// Path of the scratch file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Size of the scratch file in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Best-effort eviction of the file's pages from the page cache, so the
    /// first pass isn't served from what writing it just cached.
    fn drop_cache(&self) {
        #[cfg(target_os = "linux")]
        if let Ok(file) = File::open(&self.path) {
            use std::os::fd::AsRawFd;
            // SAFETY: `file` owns a live descriptor for the duration of the call;
            // POSIX_FADV_DONTNEED only advises the kernel to drop clean pages.
            unsafe {
                libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
            }
        }
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Write `size` bytes of incompressible-ish data to `path` and flush it to disk.
fn fill(path: &Path, size: u64) -> io::Result<()> {
    use std::io::Write;

    let file = File::create(path)?;
    // macOS keeps written data in the unified buffer cache; F_NOCACHE on the
    // writing handle keeps it out, so the read phase can't hit it.
    #[cfg(target_os = "macos")]
    set_nocache(&file);

    let mut chunk = vec![0u8; FILL_CHUNK];
    let mut acc: u64 = 0x9e37_79b9_7f4a_7c15;
    for slot in chunk.chunks_mut(8) {
        acc = acc
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        slot.copy_from_slice(&acc.to_le_bytes()[..slot.len()]);
    }

    let mut writer = std::io::BufWriter::new(&file);
    let mut written = 0u64;
    while written < size {
        let n = FILL_CHUNK.min(usize::try_from(size - written).unwrap_or(FILL_CHUNK));
        writer.write_all(&chunk[..n])?;
        written += n as u64;
    }
    writer.flush()?;
    file.sync_all()
}

/// A read handle on the scratch file, opened to bypass the cache where possible.
pub struct Reader {
    file: File,
    bypass: CacheBypass,
}

impl Reader {
    /// Open `path` for cache-bypassing reads, falling back to a buffered handle
    /// (reported as such) where the platform or filesystem refuses.
    ///
    /// # Errors
    ///
    /// Returns any I/O error from opening the file.
    pub fn open(path: &Path) -> io::Result<Self> {
        let (file, bypass) = open_bypassing(path)?;
        Ok(Self { file, bypass })
    }

    /// Whether this handle actually bypasses the cache.
    pub fn bypass(&self) -> CacheBypass {
        self.bypass
    }

    /// Read into `buf` at `offset`. Both must be [`ALIGN`]-aligned for a direct
    /// handle; [`AlignedBuf`] and [`block_offset`] guarantee that.
    ///
    /// # Errors
    ///
    /// Returns any I/O error from the underlying positional read.
    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            self.file.read_at(buf, offset)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            self.file.seek_read(buf, offset)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (buf, offset);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "positional reads unsupported on this platform",
            ))
        }
    }
}

#[cfg(target_os = "linux")]
fn open_bypassing(path: &Path) -> io::Result<(File, CacheBypass)> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;

    match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECT)
        .open(path)
    {
        Ok(file) => Ok((file, CacheBypass::Direct)),
        // tmpfs and a few filesystems reject O_DIRECT outright.
        Err(_) => Ok((File::open(path)?, CacheBypass::Buffered)),
    }
}

#[cfg(target_os = "macos")]
fn open_bypassing(path: &Path) -> io::Result<(File, CacheBypass)> {
    let file = File::open(path)?;
    let bypass = if set_nocache(&file) {
        CacheBypass::Direct
    } else {
        CacheBypass::Buffered
    };
    Ok((file, bypass))
}

/// Turn off buffer-cache retention and read-ahead for this handle.
#[cfg(target_os = "macos")]
fn set_nocache(file: &File) -> bool {
    use std::os::fd::AsRawFd;

    let fd = file.as_raw_fd();
    // SAFETY: `file` owns a live descriptor for the duration of both calls; both
    // fcntl commands take an int argument and only affect this descriptor.
    unsafe {
        let nocache = libc::fcntl(fd, libc::F_NOCACHE, 1) == 0;
        libc::fcntl(fd, libc::F_RDAHEAD, 0);
        nocache
    }
}

#[cfg(windows)]
fn open_bypassing(path: &Path) -> io::Result<(File, CacheBypass)> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    /// `FILE_FLAG_NO_BUFFERING` — reads go straight to the device, with the
    /// sector-alignment requirements [`ALIGN`] satisfies.
    const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;

    match OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_NO_BUFFERING)
        .open(path)
    {
        Ok(file) => Ok((file, CacheBypass::Direct)),
        Err(_) => Ok((File::open(path)?, CacheBypass::Buffered)),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn open_bypassing(path: &Path) -> io::Result<(File, CacheBypass)> {
    Ok((File::open(path)?, CacheBypass::Buffered))
}

/// A heap buffer whose readable slice starts on an [`ALIGN`] boundary, as direct
/// I/O requires. Over-allocates and takes an aligned window — no `unsafe`.
pub struct AlignedBuf {
    raw: Vec<u8>,
    offset: usize,
    len: usize,
}

impl AlignedBuf {
    /// Allocate a buffer exposing `len` aligned bytes.
    pub fn new(len: usize) -> Self {
        let raw = vec![0u8; len + ALIGN];
        // `align_offset` may in principle report "impossible" (usize::MAX); for a
        // heap byte allocation it never does, but fall back rather than overflow.
        let offset = match raw.as_ptr().align_offset(ALIGN) {
            o if o <= ALIGN => o,
            _ => 0,
        };
        Self { raw, offset, len }
    }

    /// The aligned window to read into.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.raw[self.offset..self.offset + self.len]
    }
}

/// The active I/O scheduler for the device backing `path`, on Linux.
///
/// Worth reporting: `none` (a common NVMe default) ignores I/O priority
/// entirely, so a null result there is the scheduler's doing, not `bgrt`'s.
#[cfg(target_os = "linux")]
pub fn io_scheduler(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;

    let dev = std::fs::metadata(path).ok()?.dev();
    let base = format!("/sys/dev/block/{}:{}", libc::major(dev), libc::minor(dev));
    // Partitions have no queue of their own; their parent device does.
    let read = |suffix: &str| std::fs::read_to_string(format!("{base}/{suffix}")).ok();
    let raw = read("queue/scheduler").or_else(|| read("../queue/scheduler"))?;
    parse_scheduler(&raw)
}

/// Non-Linux: no equivalent knob to report.
#[cfg(not(target_os = "linux"))]
pub fn io_scheduler(_path: &Path) -> Option<String> {
    None
}

/// Pick the active scheduler out of a `queue/scheduler` line, e.g.
/// `"mq-deadline kyber [bfq]"` → `"bfq"`. A single unbracketed name (as `none`
/// is printed on some kernels) is taken as active.
// Only [`io_scheduler`] calls this, and only on Linux — but as a pure function it
// is unit-tested on every platform, which is the point of splitting it out.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_scheduler(raw: &str) -> Option<String> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }
    if let Some(start) = line.find('[') {
        let rest = &line[start + 1..];
        let end = rest.find(']')?;
        let name = rest[..end].trim();
        return (!name.is_empty()).then(|| name.to_owned());
    }
    let mut names = line.split_whitespace();
    match (names.next(), names.next()) {
        (Some(only), None) => Some(only.to_owned()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "io_file_tests.rs"]
mod io_file_tests;
