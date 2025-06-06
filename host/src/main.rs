// These constants represent the RISC-V ELF and the image ID generated by risc0-build.
// The ELF is used for proving and the ID is used for verification.
use methods::{
    METHOD_ELF, METHOD_ID
};
use risc0_zkvm::{default_prover, ExecutorEnv, ProverOpts};
use risc0_zkp::core::{digest::Digest as Risc0Digest};
use risc0_binfmt::Digestible;
use sha2::{Digest, Sha256};

fn main() {
    // Initialize tracing. In order to view logs, run `RUST_LOG=info cargo run`
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    // An executor environment describes the configurations for the zkVM
    // including program inputs.
    // A default ExecutorEnv can be created like so:
    // `let env = ExecutorEnv::builder().build().unwrap();`
    // However, this `env` does not have any inputs.
    //
    // To add guest input to the executor environment, use
    // ExecutorEnvBuilder::write().
    // To access this method, you'll need to use ExecutorEnv::builder(), which
    // creates an ExecutorEnvBuilder. When you're done adding input, call
    // ExecutorEnvBuilder::build().

    // For example:
    let input: u32 = 15 * u32::pow(2, 27) + 1;
    let env = ExecutorEnv::builder()
        .write(&input)
        .unwrap()
        .build()
        .unwrap();

    // Obtain the default prover.
    let prover = default_prover();

    // Proof information by proving the specified ELF binary.
    // This struct contains the receipt along with statistics about execution of the guest
    let prove_info = prover
        .prove_with_opts(env, METHOD_ELF, &ProverOpts::groth16())
        .unwrap();

    // extract the receipt.
    let receipt = prove_info.receipt;

    // TODO: Implement code for retrieving receipt journal here.

    // For example:
    let _output: u32 = receipt.journal.decode().unwrap();

    receipt
        .verify(METHOD_ID)
        .unwrap();

    let expected_output_bytes: [u8; 31] = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&calculate_succinct_output_prefix(METHOD_ID));
        hasher.update(&receipt.journal.bytes);
        let expected_output = hasher.finalize();
        let expected_output_bytes: [u8; 32] = expected_output.into();
        expected_output_bytes[..31].try_into().unwrap()
    };

    // Verify the proof
    {
        use ark_ff::PrimeField;

        let groth_proof = receipt.inner.groth16().expect("Not a groth16 proof!!!");
        let ark_proof = from_seal(&groth_proof.seal.as_slice());
        let public_input_scalar =
            ark_bn254::Fr::from_be_bytes_mod_order(&expected_output_bytes);

        let ark_vk = get_ark_verifying_key();
        let ark_pvk = ark_groth16::prepare_verifying_key(&ark_vk);

        let res = ark_groth16::Groth16::<ark_bn254::Bn254>::verify_proof(
            &ark_pvk,
            &ark_proof,
            &[public_input_scalar],
        )
        .unwrap();

        assert!(res, "proof verification failed for receipt");
    }
}

pub fn calculate_succinct_output_prefix(method_id: &[u8]) -> [u8; 32] {
    let succinct_verifier_params = risc0_zkvm::SuccinctReceiptVerifierParameters::default();
    let succinct_control_root = succinct_verifier_params.control_root;
    let mut succinct_control_root_bytes: [u8; 32] =
        succinct_control_root.as_bytes().try_into().unwrap();
    for byte in succinct_control_root_bytes.iter_mut() {
        *byte = byte.reverse_bits();
    }
    let pre_state_bytes = method_id.to_vec();
    let control_id_bytes: [u8; 32] =
        risc0_circuit_recursion::control_id::BN254_IDENTITY_CONTROL_ID.into();

    // Expected post state for an execution that halted successfully
    let post_state = risc0_zkvm::SystemState {
        pc: 0,
        merkle_root: Risc0Digest::default(),
    };
    let post_state_bytes: [u8; 32] = post_state.digest::<risc0_zkvm::sha::Impl>().into();

    let mut hasher = Sha256::new();
    hasher.update(&succinct_control_root_bytes);
    hasher.update(&pre_state_bytes);
    hasher.update(&post_state_bytes);
    hasher.update(&control_id_bytes);
    let result: [u8; 32] = hasher
        .finalize()
        .try_into()
        .expect("SHA256 should produce a 32-byte output");

    result
}

fn from_seal(seal_bytes: &[u8]) -> ark_groth16::Proof<ark_bn254::Bn254> {
    use ark_bn254::{Fq, Fq2, G1Affine, G2Affine};
    use ark_ff::{Field, PrimeField};

    let a = G1Affine::new(
        Fq::from_be_bytes_mod_order(&seal_bytes[0..32]),
        Fq::from_be_bytes_mod_order(&seal_bytes[32..64]),
    );

    let b = G2Affine::new(
        Fq2::from_base_prime_field_elems(&[
            Fq::from_be_bytes_mod_order(&seal_bytes[96..128]),
            Fq::from_be_bytes_mod_order(&seal_bytes[64..96]),
        ])
        .unwrap(),
        Fq2::from_base_prime_field_elems(&[
            Fq::from_be_bytes_mod_order(&seal_bytes[160..192]),
            Fq::from_be_bytes_mod_order(&seal_bytes[128..160]),
        ])
        .unwrap(),
    );

    let c = G1Affine::new(
        Fq::from_be_bytes_mod_order(&seal_bytes[192..224]),
        Fq::from_be_bytes_mod_order(&seal_bytes[224..256]),
    );

    ark_groth16::Proof {
        a: a.into(),
        b: b.into(),
        c: c.into(),
    }
}

fn get_ark_verifying_key() -> ark_groth16::VerifyingKey<ark_bn254::Bn254> {
    use ark_bn254::{Bn254, Fq, Fq2, G1Affine, G2Affine};
    use std::str::FromStr;

    let alpha_g1 = G1Affine::new(
        Fq::from_str(
            "20491192805390485299153009773594534940189261866228447918068658471970481763042",
        )
        .unwrap(),
        Fq::from_str(
            "9383485363053290200918347156157836566562967994039712273449902621266178545958",
        )
        .unwrap(),
    );

    let beta_g2 = G2Affine::new(
        Fq2::new(
            Fq::from_str(
                "6375614351688725206403948262868962793625744043794305715222011528459656738731",
            )
            .unwrap(),
            Fq::from_str(
                "4252822878758300859123897981450591353533073413197771768651442665752259397132",
            )
            .unwrap(),
        ),
        Fq2::new(
            Fq::from_str(
                "10505242626370262277552901082094356697409835680220590971873171140371331206856",
            )
            .unwrap(),
            Fq::from_str(
                "21847035105528745403288232691147584728191162732299865338377159692350059136679",
            )
            .unwrap(),
        ),
    );

    let gamma_g2 = G2Affine::new(
        Fq2::new(
            Fq::from_str(
                "10857046999023057135944570762232829481370756359578518086990519993285655852781",
            )
            .unwrap(),
            Fq::from_str(
                "11559732032986387107991004021392285783925812861821192530917403151452391805634",
            )
            .unwrap(),
        ),
        Fq2::new(
            Fq::from_str(
                "8495653923123431417604973247489272438418190587263600148770280649306958101930",
            )
            .unwrap(),
            Fq::from_str(
                "4082367875863433681332203403145435568316851327593401208105741076214120093531",
            )
            .unwrap(),
        ),
    );

    let delta_g2 = G2Affine::new(
        Fq2::new(
            Fq::from_str(
                "19928663713463533589216209779412278386769407450988172849262535478593422929698",
            )
            .unwrap(),
            Fq::from_str(
                "19916519943909223643323234301580053157586699704876134064841182937085943926141",
            )
            .unwrap(),
        ),
        Fq2::new(
            Fq::from_str(
                "4584600978911428195337731119171761277167808711062125916470525050324985708782",
            )
            .unwrap(),
            Fq::from_str(
                "903010326261527050999816348900764705196723158942686053018929539519969664840",
            )
            .unwrap(),
        ),
    );

    let gamma_abc_g1 = vec![
        G1Affine::new(
            Fq::from_str(
                "6698887085900109660417671413804888867145870700073340970189635830129386206569",
            )
            .unwrap(),
            Fq::from_str(
                "10431087902009508261375793061696708147989126018612269070732549055898651692604",
            )
            .unwrap(),
        ),
        G1Affine::new(
            Fq::from_str(
                "20225609417084538563062516991929114218412992453664808591983416996515711931386",
            )
            .unwrap(),
            Fq::from_str(
                "3236310410959095762960658876334609343091075204896196791007975095263664214628",
            )
            .unwrap(),
        ),
    ];

    ark_groth16::VerifyingKey::<Bn254> {
        alpha_g1,
        beta_g2,
        gamma_g2,
        delta_g2,
        gamma_abc_g1,
    }
}

