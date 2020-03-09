use std::borrow::Cow;

use algebra::curves::bls12_377::{Bls12_377Parameters, G1Projective, G2Projective};
use algebra::fields::bls12_377::fq::Fq;
use algebra::fields::sw6::Fr as SW6Fr;
use algebra::ProjectiveCurve;
use crypto_primitives::crh::pedersen::PedersenParameters;
use crypto_primitives::FixedLengthCRHGadget;
use r1cs_core::{ConstraintSynthesizer, ConstraintSystem, SynthesisError};
use r1cs_std::fields::fp::FpGadget;
use r1cs_std::groups::curves::short_weierstrass::bls12::{G1Gadget, G2Gadget};
use r1cs_std::prelude::*;

use crate::gadgets::constant::AllocConstantGadget;
use crate::gadgets::macro_block::{CRHGadget, MacroBlockGadget};
use crate::macro_block::MacroBlock;
use crate::setup::{CRH, G1_GENERATOR2, G2_GENERATOR};
use crate::{end_cost_analysis, next_cost_analysis, start_cost_analysis};

pub struct Circuit {
    prev_keys: Vec<G2Projective>,
    block_number: u32,
    block: MacroBlock,
    generator: G2Projective,
    max_non_signers: u64,
    crh_parameters: PedersenParameters<G1Projective>,

    // Public input
    state_hash: G1Projective,
}

impl Circuit {
    pub const EPOCH_LENGTH: u32 = 10;

    /// `min_signers` enforces the number of signers to be >= `min_signers`.
    pub fn new(
        prev_keys: Vec<G2Projective>,
        mut block: MacroBlock,
        crh_parameters: PedersenParameters<G1Projective>,
        min_signers: usize,
        state_hash: G1Projective,
    ) -> Self {
        Self {
            prev_keys,
            block_number: 0,
            block,
            generator: G2Projective::prime_subgroup_generator(),
            // non < max_excl <=> signers >= min_incl => max_excl = (SLOTS - min_incl + 1)
            max_non_signers: (MacroBlock::SLOTS - min_signers + 1) as u64,
            crh_parameters,
            state_hash,
        }
    }
}

impl ConstraintSynthesizer<Fq> for Circuit {
    fn generate_constraints<CS: ConstraintSystem<Fq>>(
        self,
        cs: &mut CS,
    ) -> Result<(), SynthesisError> {
        #[allow(unused_mut)]
        let mut cost = start_cost_analysis!(cs, || "Alloc public input");
        let state_hash =
            G1Gadget::<Bls12_377Parameters>::alloc_input(cs.ns(|| "state hash"), || {
                Ok(&self.state_hash)
            })?;

        next_cost_analysis!(cs, cost, || "Alloc private input & constants");
        // The block number is part of the hash.
        let epoch_length = UInt32::constant(Self::EPOCH_LENGTH);

        let prev_keys_var = Vec::<G2Gadget<Bls12_377Parameters>>::alloc(
            cs.ns(|| "previous keys"),
            &self.prev_keys[..],
        )?;

        let block_number_var = UInt32::alloc(cs.ns(|| "block number"), Some(self.block_number))?;

        let mut block_var =
            MacroBlockGadget::alloc(cs.ns(|| "macro blocks"), || Ok(&self.blocks[..]))?;

        let max_non_signers_var: FpGadget<SW6Fr> = AllocConstantGadget::alloc_const(
            cs.ns(|| "max non signers"),
            &SW6Fr::from(self.max_non_signers),
        )?;

        let generator_var: G2Gadget<Bls12_377Parameters> =
            AllocConstantGadget::alloc_const(cs.ns(|| "generator"), &self.generator)?;

        let sum_generator_g1_var: G1Gadget<Bls12_377Parameters> = AllocConstantGadget::alloc_const(
            cs.ns(|| "sum generator g1"),
            &G1_GENERATOR2.into_affine(),
        )?;
        let sum_generator_g2_var: G2Gadget<Bls12_377Parameters> = AllocConstantGadget::alloc_const(
            cs.ns(|| "sum generator g2"),
            &G2_GENERATOR.into_affine(),
        )?;

        next_cost_analysis!(cs, cost, || { "Alloc parameters for Pedersen Hash" });
        let crh_parameters =
            <CRHGadget as FixedLengthCRHGadget<CRH, SW6Fr>>::ParametersGadget::alloc(
                &mut cs.ns(|| "crh_parameters"),
                || Ok(&self.crh_parameters),
            )?;

        // TODO: verify equality input state hash

        // Verify block
        next_cost_analysis!(cs, cost, || "Verify block");
        block_var.verify(
            cs.ns(|| "verify block"),
            &prev_keys,
            &max_non_signers_var,
            &block_number_var,
            &generator_var,
            &sum_generator_g1_var,
            &sum_generator_g2_var,
            &crh_parameters,
        )?;

        // TODO: Use Fq instead.
        block_number = UInt32::addmany(
            cs.ns(|| format!("block number for {}", i + 1)),
            &[block_number, epoch_length.clone()],
        )?;

        // TODO: verify equality output state hash
        // Finally verify that the last block's public key sum correspond to the public input.
        next_cost_analysis!(cs, cost, || "Public key equality with input");
        last_public_key_sum_var
            .enforce_equal(cs.ns(|| "public key equality"), &prev_public_key_sum)?;
        end_cost_analysis!(cs, cost);

        Ok(())
    }
}
