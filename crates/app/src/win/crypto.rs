//! SHA-512 through CNG (BCrypt), for update and reinstall checksums.
use super::arg;
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;
use windows::Win32::Foundation::NTSTATUS;
use windows::Win32::Security::Cryptography::{
    BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData, BCRYPT_HASH_HANDLE, BCRYPT_SHA512_ALG_HANDLE,
};

fn check(status: NTSTATUS) -> Result<()> {
    status.ok().map_err(|e| Error::msg(format!("SHA-512 failed: {}", e.message())))
}

struct Hash(BCRYPT_HASH_HANDLE);

impl Hash {
    fn new() -> Result<Hash> {
        let mut h = BCRYPT_HASH_HANDLE::default();
        // The pseudo-handle (Windows 10+) needs no provider to open or close.
        check(unsafe { BCryptCreateHash(BCRYPT_SHA512_ALG_HANDLE, &mut h, None, None, 0) })?;
        Ok(Hash(h))
    }

    fn update(&mut self, data: &[u8]) -> Result<()> {
        // BCryptHashData takes a u32 length.
        for chunk in data.chunks(u32::MAX as usize) {
            check(unsafe { BCryptHashData(self.0, chunk, 0) })?;
        }
        Ok(())
    }

    fn finish(self) -> Result<[u8; 64]> {
        let mut out = [0u8; 64];
        check(unsafe { BCryptFinishHash(self.0, &mut out, 0) })?;
        Ok(out)
    }
}

impl Drop for Hash {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptDestroyHash(self.0);
        }
    }
}

/// SHA-512 of `data`. Only fails if CNG itself does, which it doesn't on a
/// working Windows; the update code's closure wants a plain `[u8; 64]`.
pub fn sha512(data: &[u8]) -> Result<[u8; 64]> {
    let mut h = Hash::new()?;
    h.update(data)?;
    h.finish()
}

/// SHA-512 of a file, read in 1 MB chunks.
pub fn sha512_file(path: &Path) -> Result<[u8; 64]> {
    let mut file = std::fs::File::open(path)?;
    let mut h = Hash::new()?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n])?;
    }
    h.finish()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `--file P`: prints the hex SHA-512 (compare with `certutil -hashfile P SHA512`).
pub fn probe(args: &[String]) -> Result<Value> {
    let file = arg(args, "--file").ok_or_else(|| Error::msg("Usage: --probe sha512 --file <path>"))?;
    Ok(json!({ "file": file, "sha512": hex(&sha512_file(Path::new(file))?) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(
            hex(&sha512(b"").unwrap()),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
        assert_eq!(
            hex(&sha512(b"abc").unwrap()),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn file_matches_slice_across_chunks() {
        let data: Vec<u8> = (0..(3 << 20) + 17).map(|i| (i * 7) as u8).collect();
        let path = std::env::temp_dir().join(format!("launchpad-sha-test-{}.bin", std::process::id()));
        std::fs::write(&path, &data).unwrap();
        let from_file = sha512_file(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(from_file.unwrap(), sha512(&data).unwrap());
    }
}
