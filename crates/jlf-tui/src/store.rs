//! A bounded, file-backed record store.
//!
//! A long-running `tail -f | jlf tui` can see millions of records; keeping
//! every raw line in memory would grow without bound. Instead the store keeps
//! only the **head** (the first records) and the **tail** (the most recent)
//! resident — the two places you actually jump to with `g` and `G`/follow — and
//! spills the **middle** to a temp file, paging chunks back on demand behind a
//! small cache. Memory stays bounded (~`head_cap + tail_cap` records plus a few
//! cached chunks) no matter how long the stream runs; the temp file holds the
//! aged-out middle and is deleted on exit.
//!
//! Records are addressed by a stable logical index `0..len()`. `get(i)` returns
//! the record (cloning only a cheap `Rc`), reading from disk when `i` falls in
//! the spilled middle.

use std::{
    cell::RefCell,
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write},
    path::PathBuf,
    rc::Rc,
};

/// Records kept resident at the start of the stream (jumped to with `g`).
const HEAD_CAP: usize = 50_000;
/// Most-recent records kept resident (the follow/`G` window).
const TAIL_CAP: usize = 50_000;
/// Records written per spilled chunk — also the unit paged back from disk.
const CHUNK: usize = 10_000;
/// Spilled chunks kept in the in-memory page cache.
const CACHE_CHUNKS: usize = 4;

/// Where a spilled chunk lives in the temp file.
struct ChunkMeta {
    first: usize,
    offset: u64,
    count: usize,
}

/// A chunk paged back into memory.
struct CachedChunk {
    first: usize,
    records: Vec<Rc<str>>,
}

pub struct Store {
    head_cap: usize,
    tail_cap: usize,
    chunk: usize,

    /// Records `[0, min(len, head_cap))`, frozen once full.
    head: Vec<Rc<str>>,
    /// The most recent up-to-`tail_cap` records.
    tail: VecDeque<Rc<str>>,
    /// Middle records evicted from the tail but not yet flushed to a chunk.
    pending: Vec<Rc<str>>,
    /// Metadata for every chunk written to the spill file, in order.
    chunks: Vec<ChunkMeta>,
    /// Count of records already written to chunks (so `pending` starts at
    /// `head_cap + spilled`).
    spilled: usize,
    len: usize,

    path: Option<PathBuf>,
    writer: Option<BufWriter<File>>,
    file_len: u64,
    /// Reader handle and paged-chunk cache — behind `RefCell` so `get(&self)`
    /// (called during rendering) can page in and cache without a `&mut`.
    reader: RefCell<Option<File>>,
    cache: RefCell<Vec<CachedChunk>>,
}

impl Store {
    pub fn new() -> Self { Self::with_caps(HEAD_CAP, TAIL_CAP, CHUNK) }

    /// Construct with explicit caps (used by tests to exercise spilling without
    /// pushing 100k records).
    pub fn with_caps(head_cap: usize, tail_cap: usize, chunk: usize) -> Self {
        Store {
            head_cap,
            tail_cap,
            chunk,
            head: Vec::new(),
            tail: VecDeque::new(),
            pending: Vec::new(),
            chunks: Vec::new(),
            spilled: 0,
            len: 0,
            path: None,
            writer: None,
            file_len: 0,
            reader: RefCell::new(None),
            cache: RefCell::new(Vec::new()),
        }
    }

    pub fn len(&self) -> usize { self.len }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool { self.len == 0 }

    /// Append a record. Fills the head first, then the tail; once the tail is
    /// full the oldest tail record ages into the spilled middle.
    pub fn push(&mut self, line: String) {
        let rc: Rc<str> = Rc::from(line);
        self.len += 1;
        if self.head.len() < self.head_cap {
            self.head.push(rc);
            return;
        }
        self.tail.push_back(rc);
        if self.tail.len() > self.tail_cap {
            let aged = self.tail.pop_front().expect("tail just pushed");
            self.pending.push(aged);
            if self.pending.len() >= self.chunk {
                self.flush_chunk();
            }
        }
    }

    /// Fetch record `i` (`i < len()`), reading from the spill file when it lies
    /// in the middle. Returns an empty record on an out-of-range index or a
    /// read error (both rare and non-fatal for a viewer).
    pub fn get(&self, i: usize) -> Rc<str> {
        if i < self.head.len() {
            return self.head[i].clone();
        }
        let tail_start = self.len - self.tail.len();
        if i >= tail_start {
            return self.tail[i - tail_start].clone();
        }
        let pending_start = self.head_cap + self.spilled;
        if i >= pending_start {
            return self.pending[i - pending_start].clone();
        }
        self.get_spilled(i)
    }

    /// Preload the chunk covering `i` so a later `get(i)` during rendering is a
    /// cache hit (used to prefetch just off the visible edges for smooth
    /// scroll).
    pub fn prefetch(&self, i: usize) {
        let pending_start = self.head_cap + self.spilled;
        if i < self.head_cap || i >= pending_start.min(self.len) {
            return; // resident or out of the spilled range
        }
        let _ = self.get_spilled(i);
    }

    fn get_spilled(&self, i: usize) -> Rc<str> {
        let Some(ci) = self.chunk_index(i) else {
            return Rc::from("");
        };
        let first = self.chunks[ci].first;
        if let Some(hit) = self
            .cache
            .borrow()
            .iter()
            .find(|c| c.first == first)
            .and_then(|c| c.records.get(i - first).cloned())
        {
            return hit;
        }
        let records = self.load_chunk(ci);
        let rec = records
            .get(i - first)
            .cloned()
            .unwrap_or_else(|| Rc::from(""));
        let mut cache = self.cache.borrow_mut();
        cache.push(CachedChunk { first, records });
        if cache.len() > CACHE_CHUNKS {
            cache.remove(0);
        }
        rec
    }

    /// Index of the chunk containing logical record `i`, if any.
    fn chunk_index(&self, i: usize) -> Option<usize> {
        let ci = self.chunks.partition_point(|c| c.first + c.count <= i);
        (ci < self.chunks.len() && self.chunks[ci].first <= i).then_some(ci)
    }

    fn load_chunk(&self, ci: usize) -> Vec<Rc<str>> {
        let meta = &self.chunks[ci];
        let mut reader = self.reader.borrow_mut();
        let Some(path) = &self.path else {
            return Vec::new();
        };
        if reader.is_none() {
            *reader = File::open(path).ok();
        }
        let Some(file) = reader.as_mut() else {
            return Vec::new();
        };
        if file.seek(SeekFrom::Start(meta.offset)).is_err() {
            return Vec::new();
        }
        let mut br = BufReader::new(&*file);
        let mut out = Vec::with_capacity(meta.count);
        let mut line = String::new();
        for _ in 0..meta.count {
            line.clear();
            match br.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    out.push(Rc::from(line.trim_end_matches(['\n', '\r'])))
                }
            }
        }
        out
    }

    /// Write the pending records as one chunk to the spill file (created
    /// lazily), recording its byte offset so it can be paged back.
    fn flush_chunk(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if self.writer.is_none() && !self.open_spill() {
            // Couldn't open a spill file: keep the records in memory rather
            // than dropping them (bounded by however far the stream
            // runs without a usable temp dir — the safe failure
            // mode).
            return;
        }
        let writer = self.writer.as_mut().expect("just opened");
        let first = self.head_cap + self.spilled;
        let offset = self.file_len;
        let mut count = 0;
        for rec in self.pending.drain(..) {
            if writeln!(writer, "{rec}").is_err() {
                break;
            }
            self.file_len += rec.len() as u64 + 1;
            count += 1;
        }
        // Flush so the reader handle sees the bytes before we index the chunk.
        if writer.flush().is_ok() && count > 0 {
            self.chunks.push(ChunkMeta { first, offset, count });
            self.spilled += count;
        }
    }

    fn open_spill(&mut self) -> bool {
        // Unique per Store (not just per process) so multiple stores — e.g. in
        // parallel tests — never share or clobber one temp file.
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("jlf-tui-{}-{seq}.spill", std::process::id()));
        match File::create(&path) {
            Ok(f) => {
                self.writer = Some(BufWriter::new(f));
                self.path = Some(path);
                true
            }
            Err(_) => false,
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // Best-effort cleanup of the temp spill file.
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(n: usize) -> Store {
        // Small caps so a handful of records exercise head/tail/spill.
        let mut s = Store::with_caps(3, 3, 2);
        for i in 0..n {
            s.push(format!("rec{i}"));
        }
        s
    }

    #[test]
    fn all_in_memory_below_caps() {
        let s = store_with(5); // head 3 + tail 2, no spill
        assert_eq!(s.len(), 5);
        assert!(s.path.is_none(), "should not spill under head+tail cap");
        for i in 0..5 {
            assert_eq!(&*s.get(i), format!("rec{i}"));
        }
    }

    #[test]
    fn spills_and_reads_back_the_middle() {
        let s = store_with(20); // head [0,3), tail last 3, middle [3,17) on disk
        assert_eq!(s.len(), 20);
        assert!(s.path.is_some(), "should have spilled a middle");
        // Every record is retrievable in order, whichever region it lives in.
        for i in 0..20 {
            assert_eq!(&*s.get(i), format!("rec{i}"), "record {i}");
        }
    }

    #[test]
    fn random_access_across_regions() {
        let s = store_with(50);
        for &i in &[49, 0, 25, 3, 47, 4, 10, 2, 30] {
            assert_eq!(&*s.get(i), format!("rec{i}"));
        }
    }

    #[test]
    fn head_and_tail_stay_resident() {
        let s = store_with(100);
        assert_eq!(s.head.len(), 3);
        assert_eq!(s.tail.len(), 3);
        // head = first 3, tail = last 3.
        assert_eq!(&*s.get(0), "rec0");
        assert_eq!(&*s.get(99), "rec99");
    }
}
