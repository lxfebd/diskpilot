//! NTFS 卷设备的 512B 对齐读取器（AlignedVolumeReader）。
//!
//! 从 mft.rs 拆分（2026-10-12）。Windows 卷设备要求偏移/长度 512B 对齐，
//! 本读取器聚合所有底层读为对齐大读，避免 os error 87。

use std::io::{Read, Seek, SeekFrom};

use crate::mft::{NTFS_BLOCK_SIZE, VOL_BUF_CAP};

pub(crate) struct AlignedVolumeReader<R: Read + Seek> {
    inner: R,
    /// 缓冲区内已有效区间 [buf_start, buf_end)（绝对文件偏移）。
    buf_start: u64,
    buf_end: u64,
    buf: Box<[u8]>,
    pos: u64,
}

impl<R: Read + Seek> AlignedVolumeReader<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self {
            inner,
            buf_start: 0,
            buf_end: 0,
            buf: vec![0u8; VOL_BUF_CAP].into_boxed_slice(),
            pos: 0,
        }
    }

    /// 把 pos 对齐扇区起始处重新加载最多一缓冲区的数据。
    pub(crate) fn fill(&mut self, want: usize) -> std::io::Result<usize> {
        let in_buf = (self.buf_end - self.buf_start) as usize;
        if in_buf >= want {
            return Ok(in_buf);
        }
        let start = self.pos & !(NTFS_BLOCK_SIZE as u64 - 1);
        self.inner.seek(SeekFrom::Start(start))?;
        self.buf_start = start;
        let mut have = 0usize;
        while have < self.buf.len() {
            match self.inner.read(&mut self.buf[have..]) {
                Ok(0) => break,
                Ok(n) => have += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        self.buf_end = start + have as u64;
        Ok(have)
    }

    pub(crate) fn read_some(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        // 先服务缓冲区内剩余数据（若有）。
        let off = (self.pos - self.buf_start) as usize;
        if self.buf_start <= self.pos && off < (self.buf_end - self.buf_start) as usize {
            let in_buf = (self.buf_end - self.buf_start) as usize;
            let n = (in_buf - off).min(out.len());
            out[..n].copy_from_slice(&self.buf[off..off + n]);
            self.pos += n as u64;
            return Ok(n);
        }
        // 对齐大读：偏移与长度都是 512 倍数 → 直接分块下发，无缓冲拷贝。
        if self.pos.is_multiple_of(NTFS_BLOCK_SIZE as u64)
            && out.len() >= NTFS_BLOCK_SIZE
            && out.len().is_multiple_of(NTFS_BLOCK_SIZE)
        {
            self.inner.seek(SeekFrom::Start(self.pos))?;
            let mut have = 0usize;
            while have < out.len() {
                match self.inner.read(&mut out[have..]) {
                    Ok(0) => break,
                    Ok(n) => have += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            self.pos += have as u64;
            return Ok(have);
        }
        // 小读/非对齐：经对齐缓冲。
        let want = out.len().min(self.buf.len()).max(1);
        let have = self.fill(want)?;
        if have == 0 {
            return Ok(0);
        }
        let off = (self.pos - self.buf_start) as usize;
        let n = (have - off).min(out.len());
        out[..n].copy_from_slice(&self.buf[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Read for AlignedVolumeReader<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.read_some(out)
    }
}

impl<R: Read + Seek> Seek for AlignedVolumeReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let target: i128 = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
            SeekFrom::End(d) => {
                let size = self.inner.seek(SeekFrom::End(0))?;
                size as i128 + d as i128
            }
        };
        let target = u64::try_from(target.max(0))
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "negative seek"))?;
        // 目标不在当前缓冲覆盖范围 → 作废缓冲，fill() 会按新对齐位置重载。
        // 目标恰在缓冲内则保留缓冲（快路径的页式访问依赖缓存复用）。
        if target < self.buf_start || target >= self.buf_end {
            self.buf_start = 0;
            self.buf_end = 0;
        }
        self.pos = target;
        Ok(target)
    }
}
