//! Block builder

use super::genesis;
use chain;
use invoke::{Identity, Invoke};
use primitives::compact::Compact;
use primitives::hash::H256;
use rug::Integer;
use std::cell::Cell;
use verification::h_g;
use VrfPk;

thread_local! {
    pub static TIMESTAMP_COUNTER: Cell<u32> = Cell::new(0);
}

pub struct BlockHashBuilder<F = Identity> {
    callback: F,
    block: Option<chain::Block>,
}

impl BlockHashBuilder {
    pub fn new() -> Self {
        BlockHashBuilder::with_callback(Identity)
    }
}

impl<F> BlockHashBuilder<F>
where
    F: Invoke<(H256, chain::Block)>,
{
    pub fn with_callback(callback: F) -> Self {
        BlockHashBuilder {
            block: None,
            callback: callback,
        }
    }

    pub fn block(self) -> BlockBuilder<Self> {
        BlockBuilder::with_callback(self)
    }

    pub fn with_block(mut self, block: chain::Block) -> Self {
        self.block = Some(block);
        self
    }

    pub fn build(self) -> F::Result {
        let block = self
            .block
            .expect("Block is supposed to be build here to get hash");
        self.callback.invoke((block.hash(), block))
    }
}

impl<F> Invoke<chain::Block> for BlockHashBuilder<F>
where
    F: Invoke<(H256, chain::Block)>,
{
    type Result = Self;

    fn invoke(self, block: chain::Block) -> Self {
        self.with_block(block)
    }
}

pub struct BlockBuilder<F = Identity> {
    callback: F,
    header: Option<chain::BlockHeader>,
    proof: vdf::Proof,
}

impl BlockBuilder {
    pub fn new() -> Self {
        BlockBuilder::with_callback(Identity)
    }
}

impl<F> BlockBuilder<F>
where
    F: Invoke<chain::Block>,
{
    pub fn with_callback(callback: F) -> Self {
        BlockBuilder {
            callback: callback,
            header: None,
            proof: vec![],
        }
    }

    pub fn with_header(mut self, header: chain::BlockHeader) -> Self {
        self.header = Some(header);
        self
    }

    pub fn with_proof(mut self, poof: vdf::Proof) -> Self {
        self.proof = poof;
        self
    }

    pub fn with_raw(mut self, raw: &'static str) -> Self {
        let raw_block: chain::Block = raw.into();
        self.header = Some(raw_block.header().clone());
        self.proof = raw_block.proof.clone();
        self
    }

    pub fn header(self) -> BlockHeaderBuilder<Self> {
        BlockHeaderBuilder::with_callback(self)
    }

    pub fn proved(mut self) -> Self {
        if let Some(header) = self.header.clone() {
            let g = h_g(&chain::IndexedBlock::from_raw(chain::Block {
                block_header: header.clone(),
                proof: vec![],
            }));
            self.proof = vdf::prove(&g, &header.randomness, header.iterations);
        }
        self
    }

    pub fn build(self) -> F::Result {
        self.callback
            .invoke(chain::Block::new(self.header.unwrap(), self.proof))
    }
}

impl<F> Invoke<chain::BlockHeader> for BlockBuilder<F>
where
    F: Invoke<chain::Block>,
{
    type Result = Self;

    fn invoke(self, header: chain::BlockHeader) -> Self {
        self.with_header(header)
    }
}

pub struct BlockHeaderBuilder<F = Identity> {
    callback: F,
    time: u32,
    parent: H256,
    bits: Compact,
    version: u32,
    pubkey: VrfPk,
    iterations: u32,
    randomness: Integer,
}

impl<F> BlockHeaderBuilder<F>
where
    F: Invoke<chain::BlockHeader>,
{
    pub fn with_callback(callback: F) -> Self {
        BlockHeaderBuilder {
            callback: callback,
            time: TIMESTAMP_COUNTER.with(|counter| {
                let val = counter.get();
                counter.set(val + 1);
                val
            }),
            parent: 0.into(),
            bits: Compact::max_value(),
            // set to 4 to allow creating long test chains
            version: 4,
            pubkey: VrfPk::from_bytes(&[0; 32]).unwrap(),
            iterations: 0u32,
            randomness: Integer::from(0),
        }
    }

    pub fn parent(mut self, parent: H256) -> Self {
        self.parent = parent;
        self
    }

    pub fn time(mut self, time: u32) -> Self {
        self.time = time;
        self
    }

    pub fn bits(mut self, bits: Compact) -> Self {
        self.bits = bits;
        self
    }

    pub fn version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    pub fn pubkey(mut self, pubkey: VrfPk) -> Self {
        self.pubkey = pubkey;
        self
    }

    pub fn iterations(mut self, iterations: u32) -> Self {
        self.iterations = iterations;
        self
    }

    pub fn evaluated(mut self) -> Self {
        let g = h_g(&chain::IndexedBlock::from_raw(chain::Block {
            block_header: chain::BlockHeader {
                version: self.version,
                previous_header_hash: self.parent,
                time: self.time,
                bits: self.bits,
                pubkey: self.pubkey.clone(),
                iterations: self.iterations,
                randomness: self.randomness,
            },
            proof: vec![],
        }));
        self.randomness = vdf::eval(&g, self.iterations);
        self
    }

    pub fn build(self) -> F::Result {
        self.callback.invoke(chain::BlockHeader {
            time: self.time,
            previous_header_hash: self.parent,
            bits: self.bits,
            version: self.version,
            pubkey: self.pubkey,
            iterations: self.iterations,
            randomness: self.randomness,
        })
    }
}

pub fn block_builder() -> BlockBuilder {
    BlockBuilder::new()
}
pub fn block_hash_builder() -> BlockHashBuilder {
    BlockHashBuilder::new()
}

pub fn build_n_empty_blocks_from(
    n: u32,
    start_iterations: u32,
    previous: &chain::BlockHeader,
) -> Vec<chain::Block> {
    let mut result = Vec::new();
    let mut previous_hash = previous.hash();
    let end_iterations = start_iterations + n;
    for i in start_iterations..end_iterations {
        let block = block_builder()
            .header()
            .iterations(i)
            .parent(previous_hash)
            .build()
            .build();
        previous_hash = block.hash();
        result.push(block);
    }
    result
}

pub fn build_n_empty_blocks_from_genesis(n: u32, start_iterations: u32) -> Vec<chain::Block> {
    build_n_empty_blocks_from(n, start_iterations, &genesis().block_header)
}

pub fn build_n_empty_blocks(n: u32, start_iterations: u32) -> Vec<chain::Block> {
    assert!(n != 0);
    let previous = block_builder()
        .header()
        .iterations(start_iterations)
        .build()
        .build();
    let mut result = vec![previous];
    let children = build_n_empty_blocks_from(n, start_iterations + 1, &result[0].block_header);
    result.extend(children);
    result
}

#[test]
fn example1() {
    let block = BlockBuilder::new().header().time(1000).build().build();
    assert_eq!(block.header().time, 1000);
}

#[test]
fn example2() {
    let (hash, block) = block_hash_builder()
        .block()
        .header()
        .parent(H256::from(0))
        .build()
        .build()
        .build();

    assert_eq!(
        hash,
        "9fd5d5ead664fae8c2366b94b8246dc90fff44f43cee02742e4962af724da94b".into()
    );
    assert_eq!(
        block.header().previous_header_hash,
        "0000000000000000000000000000000000000000000000000000000000000000".into()
    );
}

#[test]
fn example3() {
    let (hash, _) = block_hash_builder()
        .block()
        .header()
        .parent(H256::from(0))
        .iterations(1024)
        .evaluated()
        .build()
        .build()
        .build();

    assert_eq!(
        hash,
        "7dedc4f783253c6a092842b04b4c6297237f0e92b7e340d17515166104b7e6aa".into()
    );
}

#[test]
fn example4() {
    let (hash, block) = block_hash_builder()
        .block()
        .header()
        .parent(H256::from(0))
        .iterations(1024)
        .evaluated()
        .build()
        .proved()
        .build()
        .build();

    // proof won't affect block_header_hash, so block_header_hash should be same as example3
    assert_eq!(
        hash,
        "7dedc4f783253c6a092842b04b4c6297237f0e92b7e340d17515166104b7e6aa".into()
    );
    assert_eq!(block, "04000000000000000000000000000000000000000000000000000000000000000000000000000000ffff002120000000000000000000000000000000000000000000000000000000000000000000040000fd00013a73031d5f669631d79ff9fed3a5147f5b0a8d6c4004ce4b1297eebb47c2c2e72304ea7340e1d596f05fd3e9c7334c77f78902ea9a304a48aec6f0372aeebf8d08e87861e4ea77ab1cd85590c414e0a2b36529b3d40a71a258f5e486ea0e654df8a68cac35e7f48cf6ba069f9949885bae03d290a4cb931226b56eb59a16eaa9f474fbe2b410dc922cd69ed1525758511be649fda83789ab1eca09b5f9d4c2bfb073b5af51785a5114e6f0ad2ba59e0196e84eb1aac0bcce21aca83108f0e6d7d92cd51bf3c087ac6cc39049adf0b34d252862297e2d0fee9b2214997d3994a77403a9d204faf70b8b8ac98df8d5c82b3bd34d0ff927ba1b4ecc12cd6c8abe080afd000108e9d0a9d2b6fa33649903df182c63d71590edbde3e82aaff55b17967eeebe3496d698744adedb8554f7329869ebaffa950f164f39602e007c807b9979a8617c140bc20c7e151313eb4577fb9bd4c99ec85fd1f30b62ac295aed18e7628d4cb660bbffe37a30558c66c1ed4ea4122122c1b833e3f62b177d19c07453c587afd1f0e53c44c89e69f8a69593b0b3ddbd64bc53e3b4e3e08a25ea9025c87e4040dc3b55b402b5b449d39a9d84afb724cc7663dc1186c5ca5406c038d2b3352d1416bf8766e689db8f3e513589e41818fa461a751cc7fe4d39ead54f72bacf8113fcaa94ea180ede3f9ec5e72e1004c52a835fd35617aeb783e04544bda095bea748fd00013725a0ab0b5f97d48e833b069b452815c2ae2bf48039a85d38504989c74c07844e3710667383e1384475dcc1b4b76e7d6cbc9ab58465ee5bd70b4957300169921b57f039b9cf07497c6efda3380bde598b8fac536a0d3934c656b6d017840fbb7e34d6caf90aeeba8fbebd7bb729af6b8e22d465df2991adf8900bde736932a416fe9c6368e62d180dfd1ddcc5465cfc3b6f06649a90d29a731b41f540db08439b24f8308bd4ea58c70ea3c930da9d4622d57999aa9607c315ef4fe72f57b0d9b91329a9ff9fb81f2727f617d44051cae86177a4621dcea520b91ad84b035f5666f9fa9b563397bc8c713ff5bb023a280df4893f846c97e612e2fb9842b006b0fd00016a5c68028299000d3eaa6fa30bc3324717ee215f5bbdc61f20482260c9a96fddd90ff65a104838af7542b5383ea20f8314b2bb7a6e1e479a1779a2dddac7b0ce04cb2753a521433432eaa7b3f13e0da40f94100e352c15dd73a07be506df6f5a96fdb168f6c53b86c97b0e3d3fc24de40046589879f80c0119f8157fe5aaaa2d85c0bb00c35cafb371207bbddb17911758dadbb8a3c6a2a38f45e87b9d0c065fe9f572645879fac15f9a81cf8f1aeea751fa73200bf4e4d12ecf206257e2c0a507e1b46f1a9d84127385541a8b40eb41f6acff7b5378d2c4269de771d4232f6f7bb3cc9630bcdbf9623a62ae9be43746568b8ee89d91cba4fbc11e3375a97962fd00010f8e6c6be0ebef2cf6cb96b03a2b6c22ef987a15dc41790b1ca3fc7c8e1dd0b36904610875d7bb838e6bd5b69a674c4feb6cef124801968a6c44cab1387dd0d43326c630b504725a51e0466615f638cb58eb838e0db30b5c5667d28e6e14fb9318440e7566a470b79f306f981fd13be0861c60f2b5e442f71455935bf6c7e1b23aeb53bf2b6535dd92fd7064de84684f07c28cea8d3a0fed094d3c35c778e16625b221442dba1f705970034864933b8bebaae6b5e6828dbae106ac5a5dd262418d2f716f54a7db4c96efc5a6ceb8601d7a30025f0ae71a5eb84db8bca14f02e883bc362209ebd8c56eef72947591d5702e927d5b55bef6c2ab10f1cd18260f5ffd0001870c824f31c42dd87efe2e23393a964d9663884bb07753b6c45145dd2ba49e249dbb2240fb482e29bfc31901499e0db40f26f7bfbe739c9fb0eb58c0530f3687bce164531c767a7248a2ae63e8db3657528dc3a7c483c96a614d023f71868b25bba918a2661cac7c95d02002770d94d7228489c07b74ebca055d409135610bf6c4a41a0aec97b5b5c808184583570b974af3e2289e56c4a3cef17d4954f077ac18ab3d2f91c7487ccf66ecedb50ca458bf1ec20ebb2afdcc0e903a749355a75a82280dad9633556d86e715ceef1580e5bff9553532b4af7303e13810ba4a2ba48fe8563a5d8f58d9ee86363970b92bd0a1972715bc0a959abbe1671269d96a0efd000176b1553ea5b2613a2443efb1a2c6bcf2439830c7cc75e64298450e7f5ed72fb6f1adb2c61c23e8019b28841d8c662001d54993000f80c036215693bb38730f1a72fad31be1cf0142bb096017723d1c994dc8723667fcb0bc61af37b5ad9eb2ac6e3df905a7c9dc7aab5724ff04134c454c46e08433c87e3383b98e7674dd7d5d5e681ff9df8e32afed143a248a79ac86d25f86aafc6ecb789704aa5abc76b92a601a256fde166fd2a09ce5e91527a8438040f0cfff7f697e4a7257aefa0ce68c45313b651cb2c05599a6cbf1bfda8a81b05e8e37b025b8acd57ce0e3869b8d73a802f5b7e6ec6a255e4e4bb26d16aea98a3b2e35488e2556843aad3509bf700cfd00016067621a6c918df717991266ee9547f8bb32e5ce015dd4ef6029f9cf85c06ff6b113151e7c8ed96416ad0edcea7c0822598320e52b7fd8c65eb75640dda455cb7e7b311dffa55e3469c88d2f70b1667121785b8311877a1c8b800ac23df69736228ef45e7a37a488312dcb6eb0b9b34c809db959d496c5fbcfcd0e349ad37f9045d1c307f71c7b6729797bca8ba9a5b556230836f7014d5620e8127135fe1d7f6aacc88eda1e4f1768b1ac939eb60f12b8fecc1cd77b0ca6c4d594ec337ff42b5fc18aa7718840313c8723ec4564f7b5161d9ae96731bea90d3251ee724ab435fcb7eec02d149ca260f47f2660dd0e731184d97f478cb876c6c23ae3a8d34510fd00019ec6c84b5435edafe15bd4dab83e9d29c2243f4cffbaa333943855291d42ddaf2b51fd08b7d567c2e3a61faa97da16352673b1ca9aaacfd3d73fcb74e777052ee8002ae64419f6ede9c56ad0d26f2a734a44ea800de6494b54c77c2b4c9d5ba3ca5f9705ff87c2a62eccbb9f414846522402dde27b8bf1b7925c00387c8c7ca0ed16e5ed3c8314e9ba31db606a1d36dd5591f813af74f36916f70ec4e7497074d336a9c25fc543b2e1725f9143cce182e8992180e5eddda54db2c1a22551e24938e18bc7d607c9f4f000c814bc612c9521f3ecdb316345355573d0ba473f273055d29ae245d267a182bf189761654bcc8f6485bef29302a93752604da7b9d381fd000119abb4690366fa0ceeccb22b101b19b57c05d8bfbe525c03654ec6694d651731873c56446ff278b157b1a0710df4824ebb5f4daaa9702fa4b7549af7bbae60ba4e9ebfac6d25528ad2390ce9e0a109d9fde1a78b003d68ab7d056146aeafc168888d58c1a6746933800e200b36b75971b5448095b77d01391fed6310927524f5323a2b55ca91c5472f412f510aeab3005f3cedfe6b93c00a8fc866037485d147f059bb48aeb9687a7c5294c6722e13c5bcc17edc2be04718b92db49dde8f55fac1123a17b8b439d5ada887ec8ce74077b798208d31ec300868842b9dabcd117937839deb836560a8826debab5eece02494c6183a4db3989f482edfad2b7d55d6fd0001249d8a58571e84f66b669ad1672c8cf589ff3ebd70e99a804dc2dfd5fc89094628ffe78792f778429c376331775956418716f96ae3adac950d30707d456e0d213741e9842bf191553390deb2c8518fded35c7efc08cffe77396767e649927af53eac3a3c696c2adbc473c8268ce032ac188a13336b9943bceb6940f3161d700226ed6af6780b2bbe13e4d674e4f48aa2af1a07c42f44b2f451bd4948a69ee8187febe2d262151e53b9b8ee2fcd50be5e4dade80af098b99eb07a0c2653fc58a49dd984ce14d2a06a350109baf0ef6a4ddee2062d05d6caffe47bee96840c49abe5431a2764dd2b16a2626374b96346f0dc6c28b9f964cee8100d4034cc4c27b0".into());
}
