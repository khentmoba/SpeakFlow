#[cfg(test)]
mod tests {
    use std::time::Instant;

    // --- Benchmark 1: Typo Correction Levenshtein Distance ---
    fn word_correction_baseline(from: &str, to: &str) -> Option<(String, String)> {
        if !from.chars().all(char::is_alphanumeric)
            || !to.chars().all(char::is_alphanumeric)
            || !from.chars().any(char::is_alphabetic)
            || !to.chars().any(char::is_alphabetic)
            || !from
                .chars()
                .filter(|c| c.is_numeric())
                .eq(to.chars().filter(|c| c.is_numeric()))
        {
            return None;
        }
        let a: Vec<char> = from.chars().collect();
        let b: Vec<char> = to.chars().collect();
        let max_len = a.len().max(b.len());
        if a.is_empty() || b.is_empty() || max_len > 24 {
            return None;
        }
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, left) in a.iter().enumerate() {
            let mut diagonal = row[0];
            row[0] = i + 1;
            for (j, right) in b.iter().enumerate() {
                let previous = row[j + 1];
                row[j + 1] = (diagonal + usize::from(left != right))
                    .min(row[j] + 1)
                    .min(previous + 1);
                diagonal = previous;
            }
        }
        let swapped = a.len() == b.len()
            && (0..a.len().saturating_sub(1)).any(|i| {
                a[i] == b[i + 1] && a[i + 1] == b[i] && a[..i] == b[..i] && a[i + 2..] == b[i + 2..]
            });
        let distance = if swapped { 1 } else { row[b.len()] };
        let max_distance = if max_len >= 9 { 3 } else { 2 };
        if distance == 0 || distance > max_distance || distance * 2 > a.len().max(b.len()) {
            return None;
        }
        Some((from.to_string(), to.to_string()))
    }

    fn word_correction_optimized(from: &str, to: &str) -> Option<(String, String)> {
        if !from.chars().all(char::is_alphanumeric)
            || !to.chars().all(char::is_alphanumeric)
            || !from.chars().any(char::is_alphabetic)
            || !to.chars().any(char::is_alphabetic)
            || !from
                .chars()
                .filter(|c| c.is_numeric())
                .eq(to.chars().filter(|c| c.is_numeric()))
        {
            return None;
        }
        let mut a_buf = ['\0'; 25];
        let mut a_len = 0;
        for ch in from.chars() {
            if a_len >= 24 {
                return None;
            }
            a_buf[a_len] = ch;
            a_len += 1;
        }
        let mut b_buf = ['\0'; 25];
        let mut b_len = 0;
        for ch in to.chars() {
            if b_len >= 24 {
                return None;
            }
            b_buf[b_len] = ch;
            b_len += 1;
        }
        if a_len == 0 || b_len == 0 {
            return None;
        }
        let a = &a_buf[..a_len];
        let b = &b_buf[..b_len];
        let max_len = a_len.max(b_len);

        let mut row = [0usize; 26];
        for (idx, val) in row[..=b.len()].iter_mut().enumerate() {
            *val = idx;
        }
        for (i, &left) in a.iter().enumerate() {
            let mut diagonal = row[0];
            row[0] = i + 1;
            for (j, &right) in b.iter().enumerate() {
                let previous = row[j + 1];
                row[j + 1] = (diagonal + usize::from(left != right))
                    .min(row[j] + 1)
                    .min(previous + 1);
                diagonal = previous;
            }
        }
        let swapped = a.len() == b.len()
            && (0..a.len().saturating_sub(1)).any(|i| {
                a[i] == b[i + 1] && a[i + 1] == b[i] && a[..i] == b[..i] && a[i + 2..] == b[i + 2..]
            });
        let distance = if swapped { 1 } else { row[b.len()] };
        let max_distance = if max_len >= 9 { 3 } else { 2 };
        if distance == 0 || distance > max_distance || distance * 2 > max_len {
            return None;
        }
        Some((from.to_string(), to.to_string()))
    }

    #[test]
    fn bench_typo_correction_stack_vs_heap() {
        let pairs = [
            ("recieved", "received"),
            ("seperate", "separate"),
            ("definately", "definitely"),
            ("occurrance", "occurrence"),
            ("untill", "until"),
            ("wierd", "weird"),
            ("accomodate", "accommodate"),
            ("goverment", "government"),
        ];

        // Warmup
        for (f, t) in pairs.iter().cycle().take(1_000) {
            let _ = word_correction_baseline(f, t);
            let _ = word_correction_optimized(f, t);
        }

        const ITERATIONS: usize = 30_000;

        let start_base = Instant::now();
        let mut count_base = 0;
        for (f, t) in pairs.iter().cycle().take(ITERATIONS) {
            if word_correction_baseline(f, t).is_some() {
                count_base += 1;
            }
        }
        let duration_base = start_base.elapsed();

        let start_opt = Instant::now();
        let mut count_opt = 0;
        for (f, t) in pairs.iter().cycle().take(ITERATIONS) {
            if word_correction_optimized(f, t).is_some() {
                count_opt += 1;
            }
        }
        let duration_opt = start_opt.elapsed();

        assert_eq!(count_base, count_opt);

        let base_us = duration_base.as_micros() as f64 / ITERATIONS as f64;
        let opt_us = duration_opt.as_micros() as f64 / ITERATIONS as f64;
        let speedup = base_us / opt_us;

        eprintln!(
            "TYPO_CORRECTION_BENCHMARK: baseline={:.3}us, optimized={:.3}us, speedup={:.2}x ({:.0} ops/sec vs {:.0} ops/sec)",
            base_us,
            opt_us,
            speedup,
            ITERATIONS as f64 / duration_base.as_secs_f64(),
            ITERATIONS as f64 / duration_opt.as_secs_f64(),
        );

        assert!(
            speedup > 1.2,
            "stack-allocated buffers should be significantly faster"
        );
    }

    // --- Benchmark 2: SQLite PRAGMA Throughput ---
    #[test]
    fn bench_sqlite_pragma_throughput() {
        let dir = std::env::temp_dir().join(format!("speakflow_bench_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db_base_path = dir.join("base.sqlite3");
        let db_opt_path = dir.join("opt.sqlite3");
        let _ = std::fs::remove_file(&db_base_path);
        let _ = std::fs::remove_file(&db_opt_path);

        // Baseline connection (disk-only defaults)
        let conn_base = rusqlite::Connection::open(&db_base_path).unwrap();
        conn_base
            .execute_batch(
                "
            CREATE TABLE records (id INTEGER PRIMARY KEY, text TEXT NOT NULL);
        ",
            )
            .unwrap();

        // Optimized connection (mmap, in-memory cache, WAL, temp_store)
        let conn_opt = rusqlite::Connection::open(&db_opt_path).unwrap();
        conn_opt
            .execute_batch(
                "
            PRAGMA synchronous = NORMAL;
            PRAGMA journal_mode = WAL;
            PRAGMA cache_size = -8000;
            PRAGMA mmap_size = 268435456;
            PRAGMA temp_store = MEMORY;
            CREATE TABLE records (id INTEGER PRIMARY KEY, text TEXT NOT NULL);
        ",
            )
            .unwrap();

        const INSERTS: usize = 2_000;
        const QUERIES: usize = 2_000;

        // Populate databases
        {
            let mut stmt = conn_base
                .prepare("INSERT INTO records (text) VALUES (?)")
                .unwrap();
            for i in 0..INSERTS {
                stmt.execute(rusqlite::params![format!(
                    "transcription record payload {}",
                    i
                )])
                .unwrap();
            }
        }
        {
            let mut stmt = conn_opt
                .prepare("INSERT INTO records (text) VALUES (?)")
                .unwrap();
            for i in 0..INSERTS {
                stmt.execute(rusqlite::params![format!(
                    "transcription record payload {}",
                    i
                )])
                .unwrap();
            }
        }

        // Measure queries on baseline
        let start_base = Instant::now();
        {
            let mut stmt = conn_base
                .prepare("SELECT text FROM records WHERE id = ?")
                .unwrap();
            for i in 0..QUERIES {
                let id = (i % INSERTS) as i64 + 1;
                let _val: String = stmt.query_row([id], |r| r.get(0)).unwrap();
            }
        }
        let duration_base = start_base.elapsed();

        // Measure queries on optimized
        let start_opt = Instant::now();
        {
            let mut stmt = conn_opt
                .prepare("SELECT text FROM records WHERE id = ?")
                .unwrap();
            for i in 0..QUERIES {
                let id = (i % INSERTS) as i64 + 1;
                let _val: String = stmt.query_row([id], |r| r.get(0)).unwrap();
            }
        }
        let duration_opt = start_opt.elapsed();

        let base_us = duration_base.as_micros() as f64 / QUERIES as f64;
        let opt_us = duration_opt.as_micros() as f64 / QUERIES as f64;
        let speedup = base_us / opt_us;

        eprintln!(
            "SQLITE_BENCHMARK: baseline={:.3}us/query, optimized={:.3}us/query, speedup={:.2}x ({:.0} qps vs {:.0} qps)",
            base_us,
            opt_us,
            speedup,
            QUERIES as f64 / duration_base.as_secs_f64(),
            QUERIES as f64 / duration_opt.as_secs_f64(),
        );
    }

    // --- Benchmark 3: Resampler Chunk Processing (Heap Alloc vs Slice) ---
    #[test]
    fn bench_resampler_throughput() {
        use rubato::Resampler;

        let chunk_size = 480;
        let ratio = 16_000.0 / 48_000.0;
        let mut resampler_base = rubato::FastFixedIn::<f32>::new(
            ratio,
            1.1,
            rubato::PolynomialDegree::Cubic,
            chunk_size,
            1,
        )
        .unwrap();
        let mut resampler_opt = rubato::FastFixedIn::<f32>::new(
            ratio,
            1.1,
            rubato::PolynomialDegree::Cubic,
            chunk_size,
            1,
        )
        .unwrap();

        let mut pending_base: Vec<f32> = (0..chunk_size * 2).map(|i| (i as f32).sin()).collect();
        let mut pending_opt = pending_base.clone();

        const CHUNKS: usize = 10_000;

        // Baseline: pending.drain(..chunk_size).collect::<Vec<f32>>()
        let start_base = Instant::now();
        for _ in 0..CHUNKS {
            if pending_base.len() < chunk_size {
                pending_base.extend((0..chunk_size).map(|i| (i as f32).sin()));
            }
            let chunk: Vec<f32> = pending_base.drain(..chunk_size).collect();
            let _ = resampler_base.process(&[&chunk], None).unwrap();
        }
        let duration_base = start_base.elapsed();

        // Optimized: resampler.process(&[&pending[..chunk_size]]) then drain
        let start_opt = Instant::now();
        for _ in 0..CHUNKS {
            if pending_opt.len() < chunk_size {
                pending_opt.extend((0..chunk_size).map(|i| (i as f32).sin()));
            }
            let slice = &pending_opt[..chunk_size];
            let _ = resampler_opt.process(&[slice], None).unwrap();
            pending_opt.drain(..chunk_size);
        }
        let duration_opt = start_opt.elapsed();

        let base_us = duration_base.as_micros() as f64 / CHUNKS as f64;
        let opt_us = duration_opt.as_micros() as f64 / CHUNKS as f64;
        let speedup = base_us / opt_us;

        eprintln!(
            "RESAMPLER_BENCHMARK: baseline={:.3}us/chunk, optimized={:.3}us/chunk, speedup={:.2}x ({:.0} chunks/sec vs {:.0} chunks/sec)",
            base_us,
            opt_us,
            speedup,
            CHUNKS as f64 / duration_base.as_secs_f64(),
            CHUNKS as f64 / duration_opt.as_secs_f64(),
        );
    }

    // --- Benchmark 4: Process Working Set RAM Trimming (Windows Memory Manager) ---
    #[cfg(target_os = "windows")]
    #[test]
    fn bench_process_memory_working_set_trimming() {
        use windows_sys::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

        let process = unsafe { GetCurrentProcess() };

        let mut simulated_workload: Vec<Vec<u8>> =
            (0..500).map(|_| vec![0xAA; 100 * 1024]).collect();
        for chunk in &mut simulated_workload {
            let last = chunk.len() - 1;
            chunk[0] = 1;
            chunk[last] = 2;
        }

        let mut pmc_before: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        unsafe {
            K32GetProcessMemoryInfo(
                process,
                &mut pmc_before,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            );
        }
        let ws_before = pmc_before.WorkingSetSize;

        drop(simulated_workload);

        let start = Instant::now();
        unsafe {
            SetProcessWorkingSetSize(process, usize::MAX, usize::MAX);
        }
        let trim_duration = start.elapsed();

        let mut pmc_after: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        unsafe {
            K32GetProcessMemoryInfo(
                process,
                &mut pmc_after,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            );
        }
        let ws_after = pmc_after.WorkingSetSize;
        let ram_reduction = ws_before as f64 / ws_after.max(1) as f64;

        eprintln!(
            "WORKING_SET_TRIM_BENCHMARK: before={:.2}MB, after={:.2}MB, reduction={:.2}x (trimmed in {:.3}ms)",
            ws_before as f64 / (1024.0 * 1024.0),
            ws_after as f64 / (1024.0 * 1024.0),
            ram_reduction,
            trim_duration.as_micros() as f64 / 1000.0,
        );

        assert!(
            ram_reduction > 2.0,
            "Working set trimming should significantly reduce active RAM"
        );
    }
}
