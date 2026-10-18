use std::{fs::{self,File},os::fd::AsRawFd,path::PathBuf};
extern "C" { fn posix_fadvise(fd:i32, offset:i64, length:i64, advice:i32)->i32; }
fn main()->Result<(),Box<dyn std::error::Error>>{
    let root=PathBuf::from(std::env::args().nth(1).ok_or("stage directory")?);
    let mut files=0;let mut bytes=0;
    for e in fs::read_dir(root)? {let p=e?.path();if p.to_string_lossy().ends_with(".stage.zst"){
        let f=File::open(p)?;let code=unsafe{posix_fadvise(f.as_raw_fd(),0,0,4)};
        if code!=0{return Err(format!("fadvise failed: {code}").into());}files+=1;bytes+=f.metadata()?.len();
    }}
    println!("{{\"files\":{files},\"bytes\":{bytes},\"advice\":\"Linux POSIX_FADV_DONTNEED on owned immutable stage files only\",\"errors\":0}}");Ok(())
}
