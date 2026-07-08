//! 生成代码清单

pub struct Manifest {
    pub blocks: Vec<ManifestBlock>,
}

pub struct ManifestBlock {
    pub address: u16,
    pub cycles: u32,
}