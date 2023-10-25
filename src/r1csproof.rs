#![allow(clippy::too_many_arguments)]
// use crate::custom_dense_mlpoly::{PolyEvalProof_PQX, PolyCommitment_PQX};

use crate::custom_dense_mlpoly::PolyEvalProof_PQX;

use super::commitments::{Commitments, MultiCommitGens};
use super::dense_mlpoly::{
  DensePolynomial, EqPolynomial, PolyCommitment, PolyCommitmentGens, PolyEvalProof,
};
use super::custom_dense_mlpoly::DensePolynomial_PQX;
use super::errors::ProofVerifyError;
use super::group::{CompressedGroup, GroupElement, VartimeMultiscalarMul};
use super::math::Math;
use super::nizk::{EqualityProof, KnowledgeProof, ProductProof};
use super::r1csinstance::R1CSInstance;
use super::random::RandomTape;
use super::scalar::Scalar;
use super::sparse_mlpoly::{SparsePolyEntry, SparsePolynomial};
use super::sumcheck::ZKSumcheckInstanceProof;
use super::timer::Timer;
use super::transcript::{AppendToTranscript, ProofTranscript};
use core::{iter, num};
use std::env::var;
use curve25519_dalek::ristretto::RistrettoPoint;
use merlin::Transcript;
use serde::{Deserialize, Serialize};

// TODO: Need to reduce the complexity of TAU!
// TODO: Need to reduce the complexity of Z_poly!

#[derive(Serialize, Deserialize, Debug)]
pub struct R1CSProofBlock {
  sc_proof_phase1: ZKSumcheckInstanceProof,
  claims_phase2: (
    CompressedGroup,
    CompressedGroup,
    CompressedGroup,
    CompressedGroup,
  ),
  pok_claims_phase2: (KnowledgeProof, ProductProof),
  proof_eq_sc_phase1: EqualityProof,
  sc_proof_phase2: ZKSumcheckInstanceProof,
  comm_vars_at_ry_list: Vec<CompressedGroup>,
  comm_vars_at_ry: CompressedGroup,
  proof_eval_vars_at_ry_list: Vec<PolyEvalProof>,
  proof_eq_sc_phase2: EqualityProof,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct R1CSProofConsis {
  sc_proof_phase1: ZKSumcheckInstanceProof,
  claims_phase2: (
    CompressedGroup,
    CompressedGroup,
    CompressedGroup,
    CompressedGroup,
  ),
  pok_claims_phase2: (KnowledgeProof, ProductProof),
  proof_eq_sc_phase1: EqualityProof,
  sc_proof_phase2: ZKSumcheckInstanceProof,
  comm_vars_at_ry: CompressedGroup,
  proof_eval_vars_at_ry: PolyEvalProof,
  proof_eq_sc_phase2: EqualityProof,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct R1CSProof {
  block_comm_vars_list: Vec<PolyCommitment>,
  block_comm_io_list: Vec<PolyCommitment>,
  exec_comm_io: PolyCommitment,
  block_proof: R1CSProofBlock,
  consis_proof: R1CSProofConsis
}

pub struct R1CSSumcheckGens {
  gens_1: MultiCommitGens,
  gens_3: MultiCommitGens,
  gens_4: MultiCommitGens,
}

// TODO: fix passing gens_1_ref
impl R1CSSumcheckGens {
  pub fn new(label: &'static [u8], gens_1_ref: &MultiCommitGens) -> Self {
    let gens_1 = gens_1_ref.clone();
    let gens_3 = MultiCommitGens::new(3, label);
    let gens_4 = MultiCommitGens::new(4, label);

    R1CSSumcheckGens {
      gens_1,
      gens_3,
      gens_4,
    }
  }
}

pub struct R1CSGens {
  gens_sc: R1CSSumcheckGens,
  gens_pc: PolyCommitmentGens,
}

impl R1CSGens {
  pub fn new(label: &'static [u8], _num_cons: usize, num_vars: usize) -> Self {
    let num_block_poly_vars = num_vars.log_2();
    let gens_pc = PolyCommitmentGens::new(num_block_poly_vars, label, false);
    let gens_sc = R1CSSumcheckGens::new(label, &gens_pc.gens.gens_1);
    R1CSGens { gens_sc, gens_pc }
  }
}

impl R1CSProof {
  fn block_prove_phase_one(
    num_rounds: usize,
    num_rounds_x: usize,
    num_rounds_q_max: usize,
    num_rounds_p: usize,
    num_proofs: &Vec<usize>,
    evals_tau_p: &mut DensePolynomial,
    evals_tau_q: &mut DensePolynomial,
    evals_tau_x: &mut DensePolynomial,
    evals_Az: &mut DensePolynomial_PQX,
    evals_Bz: &mut DensePolynomial_PQX,
    evals_Cz: &mut DensePolynomial_PQX,
    gens: &R1CSSumcheckGens,
    transcript: &mut Transcript,
    random_tape: &mut RandomTape,
  ) -> (ZKSumcheckInstanceProof, Vec<Scalar>, Vec<Scalar>, Scalar) {
    let comb_func = |poly_A_comp: &Scalar,
                     poly_B_comp: &Scalar,
                     poly_C_comp: &Scalar,
                     poly_D_comp: &Scalar|
     -> Scalar { poly_A_comp * (poly_B_comp * poly_C_comp - poly_D_comp) };

    let (sc_proof_phase_one, r, claims, blind_claim_postsc) =
      ZKSumcheckInstanceProof::prove_cubic_with_additive_term_disjoint_rounds(
        &Scalar::zero(), // claim is zero
        &Scalar::zero(), // blind for claim is also zero
        num_rounds,
        num_rounds_x,
        num_rounds_q_max,
        num_rounds_p,
        num_proofs.clone(),
        evals_tau_p,
        evals_tau_q,
        evals_tau_x,
        evals_Az,
        evals_Bz,
        evals_Cz,
        comb_func,
        &gens.gens_1,
        &gens.gens_4,
        transcript,
        random_tape,
      );

    (sc_proof_phase_one, r, claims, blind_claim_postsc)
  }

  fn block_prove_phase_two(
    num_rounds: usize,
    claim: &Scalar,
    blind_claim: &Scalar,
    evals_z: &mut DensePolynomial,
    evals_ABC: &mut DensePolynomial,
    evals_eq: &mut DensePolynomial,
    gens: &R1CSSumcheckGens,
    transcript: &mut Transcript,
    random_tape: &mut RandomTape,
  ) -> (ZKSumcheckInstanceProof, Vec<Scalar>, Vec<Scalar>, Scalar) {
    let comb_func =
      |poly_A_comp: &Scalar, poly_B_comp: &Scalar, poly_C_comp: &Scalar| -> Scalar { poly_A_comp * poly_B_comp * poly_C_comp };
    let (sc_proof_phase_two, r, claims, blind_claim_postsc) = ZKSumcheckInstanceProof::prove_cubic(
      claim,
      blind_claim,
      num_rounds,
      evals_z,
      evals_ABC,
      evals_eq,
      comb_func,
      &gens.gens_1,
      &gens.gens_4,
      transcript,
      random_tape,
    );

    (sc_proof_phase_two, r, claims, blind_claim_postsc)
  }

  fn consis_prove_phase_one(
    num_rounds: usize,
    evals_tau: &mut DensePolynomial,
    evals_Az: &mut DensePolynomial,
    evals_Bz: &mut DensePolynomial,
    evals_Cz: &mut DensePolynomial,
    gens: &R1CSSumcheckGens,
    transcript: &mut Transcript,
    random_tape: &mut RandomTape,
  ) -> (ZKSumcheckInstanceProof, Vec<Scalar>, Vec<Scalar>, Scalar) {
    let comb_func = |poly_A_comp: &Scalar,
                     poly_B_comp: &Scalar,
                     poly_C_comp: &Scalar,
                     poly_D_comp: &Scalar|
     -> Scalar { poly_A_comp * (poly_B_comp * poly_C_comp - poly_D_comp) };

    let (sc_proof_phase_one, r, claims, blind_claim_postsc) =
      ZKSumcheckInstanceProof::prove_cubic_with_additive_term(
        &Scalar::zero(), // claim is zero
        &Scalar::zero(), // blind for claim is also zero
        num_rounds,
        evals_tau,
        evals_Az,
        evals_Bz,
        evals_Cz,
        comb_func,
        &gens.gens_1,
        &gens.gens_4,
        transcript,
        random_tape,
      );

    (sc_proof_phase_one, r, claims, blind_claim_postsc)
  }

  fn consis_prove_phase_two(
    num_rounds: usize,
    claim: &Scalar,
    blind_claim: &Scalar,
    evals_z: &mut DensePolynomial,
    evals_ABC: &mut DensePolynomial,
    gens: &R1CSSumcheckGens,
    transcript: &mut Transcript,
    random_tape: &mut RandomTape,
  ) -> (ZKSumcheckInstanceProof, Vec<Scalar>, Vec<Scalar>, Scalar) {
    let comb_func =
      |poly_A_comp: &Scalar, poly_B_comp: &Scalar| -> Scalar { poly_A_comp * poly_B_comp };
    let (sc_proof_phase_two, r, claims, blind_claim_postsc) = ZKSumcheckInstanceProof::prove_quad(
      claim,
      blind_claim,
      num_rounds,
      evals_z,
      evals_ABC,
      comb_func,
      &gens.gens_1,
      &gens.gens_3,
      transcript,
      random_tape,
    );

    (sc_proof_phase_two, r, claims, blind_claim_postsc)
  }


  fn protocol_name() -> &'static [u8] {
    b"R1CS proof"
  }

  pub fn prove(
    block_max_num_proofs: usize,
    block_num_proofs: &Vec<usize>,
    block_inst: &R1CSInstance,
    block_gens: &R1CSGens,
    consis_num_proofs: usize,
    consis_inst: &R1CSInstance,
    consis_gens: &R1CSGens,
    block_vars_mat: Vec<Vec<Vec<Scalar>>>,
    block_inputs_mat: Vec<Vec<Vec<Scalar>>>,
    exec_inputs: Vec<Vec<Scalar>>,
    transcript: &mut Transcript,
    random_tape: &mut RandomTape,
  ) -> (R1CSProof, [Vec<Scalar>; 4], [Vec<Scalar>; 3]) { 
    let timer_prove = Timer::new("R1CSProof::prove");
    transcript.append_protocol_name(R1CSProof::protocol_name());

    let block_num_instances = block_vars_mat.len();
    assert_eq!(block_num_instances.next_power_of_two(), block_num_instances);

    assert_eq!(block_max_num_proofs.next_power_of_two(), block_max_num_proofs);
    for i in 0..block_num_instances {
      assert!(block_vars_mat[i].len() <= block_max_num_proofs);
      assert!(block_vars_mat[i].len() == block_vars_mat[i].len().next_power_of_two());
    }

    // --
    // COMMITMENTS
    // --

    let (
      block_poly_vars_list,
      block_comm_vars_list,
      block_poly_io_list,
      block_comm_io_list,
      exec_poly_io, 
      exec_comm_io
    ) = {
      let timer_commit = Timer::new("polycommit");
      // commit the witnesses and inputs separately instance-by-instance
      let mut block_poly_vars_list = Vec::new();
      let mut block_comm_vars_list = Vec::new();
      let mut block_poly_io_list = Vec::new();
      let mut block_comm_io_list = Vec::new();

      for p in 0..block_num_instances {
        let (block_poly_vars, block_comm_vars) = {
          // Flatten the witnesses into a Q_i * X list
          let vars_list_p = block_vars_mat[p].iter().fold(Vec::new(), |a, b| [a, b.to_vec()].concat());
          // create a multilinear polynomial using the supplied assignment for variables
          let block_poly_vars = DensePolynomial::new(vars_list_p);

          // produce a commitment to the satisfying assignment
          let (block_comm_vars, _blinds_vars) = block_poly_vars.commit(&block_gens.gens_pc, None);

          // add the commitment to the prover's transcript
          block_comm_vars.append_to_transcript(b"poly_commitment", transcript);
          (block_poly_vars, block_comm_vars)
        };
        let (block_poly_io, block_comm_io) = {
          // Flatten the inputs into a Q_i * X list
          let io_list_p = block_inputs_mat[p].iter().fold(Vec::new(), |a, b| [a, b.to_vec()].concat());
          // create a multilinear polynomial using the supplied assignment for variables
          let block_poly_io = DensePolynomial::new(io_list_p);

          // produce a commitment to the satisfying assignment
          let (block_comm_io, _blinds_io) = block_poly_io.commit(&block_gens.gens_pc, None);

          // add the commitment to the prover's transcript
          block_comm_io.append_to_transcript(b"poly_commitment", transcript);
          (block_poly_io, block_comm_io)
        };
        block_poly_vars_list.push(block_poly_vars);
        block_comm_vars_list.push(block_comm_vars);
        block_poly_io_list.push(block_poly_io);
        block_comm_io_list.push(block_comm_io);
      }

      let (exec_poly_io, exec_comm_io) = {
        let exec_io_list = exec_inputs.iter().fold(Vec::new(), |a, b| [a, b.to_vec()].concat());
        // create a multilinear polynomial using the supplied assignment for variables
        let exec_poly_io = DensePolynomial::new(exec_io_list);

        // produce a commitment to the satisfying assignment
        let (exec_comm_io, _blinds_io) = exec_poly_io.commit(&consis_gens.gens_pc, None);

        // add the commitment to the prover's transcript
        exec_comm_io.append_to_transcript(b"poly_commitment", transcript);
        (exec_poly_io, exec_comm_io)
      };
      timer_commit.stop();

      (
        block_poly_vars_list,
        block_comm_vars_list,
        block_poly_io_list,
        block_comm_io_list,
        exec_poly_io, 
        exec_comm_io
      )
    };

    // --
    // BLOCK CORRECTNESS
    // --
    let (block_proof, block_challenges) = {
      let gens = block_gens;
      let num_cons = block_inst.get_num_cons();
      let timer_sc_proof_phase1 = Timer::new("prove_sc_phase_one");

      // append input to variables to create a single vector z
      let mut z_mat = Vec::new();
      for i in 0..block_num_instances {
        z_mat.push(Vec::new());
        for j in 0..block_vars_mat[i].len() {
          z_mat[i].push([block_vars_mat[i][j].clone(), block_inputs_mat[i][j].clone()].concat());
        }
      }
      let z_len = z_mat[0][0].len();

      // derive the verifier's challenge \tau
      let (num_rounds_p, num_rounds_q, num_rounds_x, num_rounds_y) = 
        (block_num_instances.log_2(), block_max_num_proofs.log_2(), num_cons.log_2(), z_len.log_2());
      let tau_p = transcript.challenge_vector(b"challenge_tau_p", num_rounds_p);
      let tau_q = transcript.challenge_vector(b"challenge_tau_q", num_rounds_q);
      let tau_x = transcript.challenge_vector(b"challenge_tau_x", num_rounds_x);

      // compute the initial evaluation table for R(\tau, x)
      let mut poly_tau_p = DensePolynomial::new(EqPolynomial::new(tau_p).evals());
      let mut poly_tau_q = DensePolynomial::new(EqPolynomial::new(tau_q).evals());
      let mut poly_tau_x = DensePolynomial::new(EqPolynomial::new(tau_x).evals());
      let (mut poly_Az, mut poly_Bz, mut poly_Cz) =
        block_inst.multiply_vec_block(block_num_instances, block_num_proofs, block_max_num_proofs, num_cons, z_len, &z_mat);

      // Sumcheck 1: (Az * Bz - Cz) * eq(x, q, p) = 0
      let (sc_proof_phase1, rx, _claims_phase1, blind_claim_postsc1) = R1CSProof::block_prove_phase_one(
        num_rounds_x + num_rounds_q + num_rounds_p,
        num_rounds_x,
        num_rounds_q,
        num_rounds_p,
        &block_num_proofs,
        &mut poly_tau_p,
        &mut poly_tau_q,
        &mut poly_tau_x,
        &mut poly_Az,
        &mut poly_Bz,
        &mut poly_Cz,
        &gens.gens_sc,
        transcript,
        random_tape,
      );
      assert_eq!(poly_tau_p.len(), 1);
      assert_eq!(poly_tau_q.len(), 1);
      assert_eq!(poly_tau_x.len(), 1);
      assert_eq!(poly_Az.len(), 1);
      assert_eq!(poly_Bz.len(), 1);
      assert_eq!(poly_Cz.len(), 1);
      timer_sc_proof_phase1.stop();

      let (tau_claim, Az_claim, Bz_claim, Cz_claim) =
        (&(poly_tau_p[0] * poly_tau_q[0] * poly_tau_x[0]), &poly_Az.index(0, 0, 0), &poly_Bz.index(0, 0, 0), &poly_Cz.index(0, 0, 0));

      let (Az_blind, Bz_blind, Cz_blind, prod_Az_Bz_blind) = (
        random_tape.random_scalar(b"Az_blind"),
        random_tape.random_scalar(b"Bz_blind"),
        random_tape.random_scalar(b"Cz_blind"),
        random_tape.random_scalar(b"prod_Az_Bz_blind"),
      );

      let (pok_Cz_claim, comm_Cz_claim) = {
        KnowledgeProof::prove(
          &gens.gens_sc.gens_1,
          transcript,
          random_tape,
          Cz_claim,
          &Cz_blind,
        )
      };

      let (proof_prod, comm_Az_claim, comm_Bz_claim, comm_prod_Az_Bz_claims) = {
        let prod = Az_claim * Bz_claim;
        ProductProof::prove(
          &gens.gens_sc.gens_1,
          transcript,
          random_tape,
          Az_claim,
          &Az_blind,
          Bz_claim,
          &Bz_blind,
          &prod,
          &prod_Az_Bz_blind,
        )
      };

      comm_Az_claim.append_to_transcript(b"comm_Az_claim", transcript);
      comm_Bz_claim.append_to_transcript(b"comm_Bz_claim", transcript);
      comm_Cz_claim.append_to_transcript(b"comm_Cz_claim", transcript);
      comm_prod_Az_Bz_claims.append_to_transcript(b"comm_prod_Az_Bz_claims", transcript);

      // prove the final step of sum-check #1
      let taus_bound_rx = tau_claim;

      let blind_expected_claim_postsc1 = taus_bound_rx * (prod_Az_Bz_blind - Cz_blind);
      let claim_post_phase1 = (Az_claim * Bz_claim - Cz_claim) * taus_bound_rx;
      let (proof_eq_sc_phase1, _C1, _C2) = EqualityProof::prove(
        &gens.gens_sc.gens_1,
        transcript,
        random_tape,
        &claim_post_phase1,
        &blind_expected_claim_postsc1,
        &claim_post_phase1,
        &blind_claim_postsc1,
      );

      // Separate the result rx into rp, rq, and rx
      let (rx, rq_rev) = rx.split_at(num_rounds_x);
      let (rq_rev, rp) = rq_rev.split_at(num_rounds_q);
      let rx = rx.to_vec();
      let rq_rev = rq_rev.to_vec();
      let rq: Vec<Scalar> = rq_rev.iter().copied().rev().collect();
      let rp = rp.to_vec();

      let timer_sc_proof_phase2 = Timer::new("prove_sc_phase_two");
      // combine the three claims into a single claim
      let r_A = transcript.challenge_scalar(b"challenge_Az");
      let r_B = transcript.challenge_scalar(b"challenge_Bz");
      let r_C = transcript.challenge_scalar(b"challenge_Cz");

      let claim_phase2 = r_A * Az_claim + r_B * Bz_claim + r_C * Cz_claim;
      let blind_claim_phase2 = r_A * Az_blind + r_B * Bz_blind + r_C * Cz_blind;

      let evals_ABC = {
        // compute the initial evaluation table for R(\tau, x)
        let evals_rx = EqPolynomial::new(rx.clone()).evals();
        let (evals_A, evals_B, evals_C) =
        block_inst.compute_eval_table_sparse(block_inst.get_num_cons(), z_len, &evals_rx);

        assert_eq!(evals_A.len(), evals_B.len());
        assert_eq!(evals_A.len(), evals_C.len());
        (0..evals_A.len())
          .map(|i| r_A * evals_A[i] + r_B * evals_B[i] + r_C * evals_C[i])
          .collect::<Vec<Scalar>>()
      };
      let mut ABC_poly = DensePolynomial::new(evals_ABC);

      // Construct a p * q * len(z) matrix Z and bound it to r_q
      let mut Z = DensePolynomial_PQX::new_rev(&z_mat, &block_num_proofs, block_max_num_proofs);
      Z.bound_poly_vars_rq(&rq_rev.to_vec());
      let mut Z_poly = Z.to_dense_poly();

      // An Eq function to match p with rp
      let mut eq_p_rp_poly = DensePolynomial::new(EqPolynomial::new(rp).evals_front(num_rounds_p + num_rounds_y));

      // Sumcheck 2: (rA + rB + rC) * Z * eq(p) = e
      let (sc_proof_phase2, ry, claims_phase2, blind_claim_postsc2) = R1CSProof::block_prove_phase_two(
        num_rounds_p + num_rounds_y,
        &claim_phase2,
        &blind_claim_phase2,
        &mut Z_poly,
        &mut ABC_poly,
        &mut eq_p_rp_poly,
        &gens.gens_sc,
        transcript,
        random_tape,
      );
      timer_sc_proof_phase2.stop();

      // Separate ry into rp and ry
      let (rp, ry) = ry.split_at(num_rounds_p);
      let rp = rp.to_vec();
      let ry = ry.to_vec();

      assert_eq!(Z_poly.len(), 1);
      assert_eq!(ABC_poly.len(), 1);

      // Bind the witnesses and inputs instance-by-instance
      let mut eval_vars_at_ry_list = Vec::new();
      let mut proof_eval_vars_at_ry_list = Vec::new();
      let mut comm_vars_at_ry_list = Vec::new();
      let timer_polyeval = Timer::new("polyeval");
      for p in 0..block_num_instances {
        // Compute combined_poly as (Scalar::one() - ry[0]) * block_poly_vars + ry[0] * block_poly_io
        let combined_poly = DensePolynomial::new(
          (0..block_poly_vars_list[p].len()).map(
            |i| (Scalar::one() - ry[0]) * block_poly_vars_list[p][i] + ry[0] * block_poly_io_list[p][i]).collect());

        // if num_proofs[p] < max_num_proofs, then only the last few entries of rq needs to be binded
        let rq_short = &rq[num_rounds_q - block_num_proofs[p].log_2()..];
        let r = [rq_short, &ry[1..]].concat();
        let eval_vars_at_ry = combined_poly.evaluate(&r);

        let (proof_eval_vars_at_ry, comm_vars_at_ry) = PolyEvalProof::prove(
          &combined_poly,
          None,
          &r,
          &eval_vars_at_ry,
          None,
          &gens.gens_pc,
          transcript,
          random_tape,
        );

        eval_vars_at_ry_list.push(eval_vars_at_ry);
        proof_eval_vars_at_ry_list.push(proof_eval_vars_at_ry);
        comm_vars_at_ry_list.push(comm_vars_at_ry);
      }
      timer_polyeval.stop();

      // Bind the resulting witness list to rp
      // block_poly_vars stores the result of each witness matrix bounded to (rq_short ++ ry)
      // but we want to bound them to (rq ++ ry)
      // So we need to multiply each entry by (1 - rq0)(1 - rq1)...
      for p in 0..block_num_instances {
        for q in 0..(num_rounds_q - block_num_proofs[p].log_2()) {
          eval_vars_at_ry_list[p] *= Scalar::one() - rq[q];
        }
      }
      let block_poly_vars = DensePolynomial::new(eval_vars_at_ry_list);
      let eval_vars_at_ry = block_poly_vars.evaluate(&rp);
      let comm_vars_at_ry = eval_vars_at_ry.commit(&Scalar::zero(), &gens.gens_pc.gens.gens_1).compress();

      // prove the final step of sum-check #2
      let blind_expected_claim_postsc2 = Scalar::zero();
      let claim_post_phase2 = claims_phase2[0] * claims_phase2[1] * claims_phase2[2];

      let (proof_eq_sc_phase2, _C1, _C2) = EqualityProof::prove(
        &gens.gens_pc.gens.gens_1,
        transcript,
        random_tape,
        &claim_post_phase2,
        &blind_expected_claim_postsc2,
        &claim_post_phase2,
        &blind_claim_postsc2,
      );

      timer_prove.stop();

      let claims_phase2 = (
        comm_Az_claim,
        comm_Bz_claim,
        comm_Cz_claim,
        comm_prod_Az_Bz_claims,
      );
      let pok_claims_phase2 = (
        pok_Cz_claim, proof_prod
      );

      (
        R1CSProofBlock {
          sc_proof_phase1,
          claims_phase2,
          pok_claims_phase2,
          proof_eq_sc_phase1,
          sc_proof_phase2,
          comm_vars_at_ry_list,
          comm_vars_at_ry,
          proof_eval_vars_at_ry_list,
          proof_eq_sc_phase2
        },
        [rp, rq_rev, rx, ry]
      )
    };

    // --
    // CONSISTENCY
    // --
    let (consis_proof, consis_challenges) = {
      let gens = consis_gens;
      let num_cons = consis_inst.get_num_cons();
      let timer_sc_proof_phase1 = Timer::new("prove_sc_phase_one");

      let num_inputs = exec_inputs[0].len();
      // interpolate exec_inputs to get its evaluation on the next num_inputs point
      fn to_binary_scalars(x: usize) -> Vec<Scalar> {
        (0..x.next_power_of_two().log_2()).rev().map(|n| (x >> n) & 1).map(|i| if i == 0 { Scalar::zero() } else { Scalar::one() }).collect()
      }
      // each z is consisted of two consecutive exec_inputs
      let mut z_list = Vec::new();
      for i in 0..consis_num_proofs - 1 {
        z_list.push([exec_inputs[i].clone(), exec_inputs[i + 1].clone()].concat());
      }
      z_list.push(
        [exec_inputs[consis_num_proofs - 1].clone(),
          (consis_num_proofs * num_inputs .. (consis_num_proofs + 1) * num_inputs).map(
            |i| exec_poly_io.evaluate(&to_binary_scalars(i))
          ).collect()
        ].concat()
      );
      let z_len = z_list[0].len();

      // derive the verifier's challenge \tau, this time without p
      let (num_rounds_q, num_rounds_x, num_rounds_y) = 
        (consis_num_proofs.log_2(), num_cons.log_2(), z_len.log_2());
      let tau = transcript.challenge_vector(b"challenge_tau", num_rounds_q + num_rounds_x);

      // compute the initial evaluation table for R(\tau, x)
      let mut poly_tau = DensePolynomial::new(EqPolynomial::new(tau).evals());
      let (mut poly_Az, mut poly_Bz, mut poly_Cz) =
        block_inst.multiply_vec_consis(0, num_cons, z_len, &z_list);

      // Sumcheck 1: (Az * Bz - Cz) * eq(x, q, p) = 0
      let (sc_proof_phase1, rx, _claims_phase1, blind_claim_postsc1) = R1CSProof::consis_prove_phase_one(
        num_rounds_q + num_rounds_x,
        &mut poly_tau,
        &mut poly_Az,
        &mut poly_Bz,
        &mut poly_Cz,
        &consis_gens.gens_sc,
        transcript,
        random_tape,
      );
      assert_eq!(poly_tau.len(), 1);
      assert_eq!(poly_Az.len(), 1);
      assert_eq!(poly_Bz.len(), 1);
      assert_eq!(poly_Cz.len(), 1);
      timer_sc_proof_phase1.stop();

      let (tau_claim, Az_claim, Bz_claim, Cz_claim) =
        (&poly_tau[0], &poly_Az[0], &poly_Bz[0], &poly_Cz[0]);

      let (Az_blind, Bz_blind, Cz_blind, prod_Az_Bz_blind) = (
        random_tape.random_scalar(b"Az_blind"),
        random_tape.random_scalar(b"Bz_blind"),
        random_tape.random_scalar(b"Cz_blind"),
        random_tape.random_scalar(b"prod_Az_Bz_blind"),
      );

      let (pok_Cz_claim, comm_Cz_claim) = {
        KnowledgeProof::prove(
          &gens.gens_sc.gens_1,
          transcript,
          random_tape,
          Cz_claim,
          &Cz_blind,
        )
      };

      let (proof_prod, comm_Az_claim, comm_Bz_claim, comm_prod_Az_Bz_claims) = {
        let prod = Az_claim * Bz_claim;
        ProductProof::prove(
          &gens.gens_sc.gens_1,
          transcript,
          random_tape,
          Az_claim,
          &Az_blind,
          Bz_claim,
          &Bz_blind,
          &prod,
          &prod_Az_Bz_blind,
        )
      };

      comm_Az_claim.append_to_transcript(b"comm_Az_claim", transcript);
      comm_Bz_claim.append_to_transcript(b"comm_Bz_claim", transcript);
      comm_Cz_claim.append_to_transcript(b"comm_Cz_claim", transcript);
      comm_prod_Az_Bz_claims.append_to_transcript(b"comm_prod_Az_Bz_claims", transcript);

      // prove the final step of sum-check #1
      let taus_bound_rx = tau_claim;

      let blind_expected_claim_postsc1 = taus_bound_rx * (prod_Az_Bz_blind - Cz_blind);
      let claim_post_phase1 = (Az_claim * Bz_claim - Cz_claim) * taus_bound_rx;
      let (proof_eq_sc_phase1, _C1, _C2) = EqualityProof::prove(
        &gens.gens_sc.gens_1,
        transcript,
        random_tape,
        &claim_post_phase1,
        &blind_expected_claim_postsc1,
        &claim_post_phase1,
        &blind_claim_postsc1,
      );

      // Separate the result rx into rp, rq, and rx
      let (rq, rx) = rx.split_at(num_rounds_q);
      let rq = rq.to_vec();
      let rx = rx.to_vec();

      let timer_sc_proof_phase2 = Timer::new("prove_sc_phase_two");
      // combine the three claims into a single claim
      let r_A = transcript.challenge_scalar(b"challenge_Az");
      let r_B = transcript.challenge_scalar(b"challenge_Bz");
      let r_C = transcript.challenge_scalar(b"challenge_Cz");

      let claim_phase2 = r_A * Az_claim + r_B * Bz_claim + r_C * Cz_claim;
      let blind_claim_phase2 = r_A * Az_blind + r_B * Bz_blind + r_C * Cz_blind;

      let evals_ABC = {
        // compute the initial evaluation table for R(\tau, x)
        let evals_rx = EqPolynomial::new(rx.clone()).evals();
        let (evals_A, evals_B, evals_C) =
        consis_inst.compute_eval_table_sparse(block_inst.get_num_cons(), z_len, &evals_rx);

        assert_eq!(evals_A.len(), evals_B.len());
        assert_eq!(evals_A.len(), evals_C.len());
        (0..evals_A.len())
          .map(|i| r_A * evals_A[i] + r_B * evals_B[i] + r_C * evals_C[i])
          .collect::<Vec<Scalar>>()
      };
      let mut ABC_poly = DensePolynomial::new(evals_ABC);

      // Construct a q * len(z) matrix Z and bound it to r_q
      let Z = z_list.iter().fold(Vec::new(), |a, b| [a, b.to_vec()].concat());
      let mut Z_poly = DensePolynomial::new(Z);
      for r in &rq {
        Z_poly.bound_poly_var_top(r);
      }

      // Sumcheck 2: (rA + rB + rC) * Z * eq(p) = e
      let (sc_proof_phase2, ry, claims_phase2, blind_claim_postsc2) = R1CSProof::consis_prove_phase_two(
        num_rounds_y,
        &claim_phase2,
        &blind_claim_phase2,
        &mut Z_poly,
        &mut ABC_poly,
        &gens.gens_sc,
        transcript,
        random_tape,
      );
      timer_sc_proof_phase2.stop();

      assert_eq!(Z_poly.len(), 1);
      assert_eq!(ABC_poly.len(), 1);

      let timer_polyeval = Timer::new("polyeval");
      // Compute combined_poly as (Scalar::one() - ry[0]) * exec_poly_io[i] + ry[0] * exec_poly_io[i + 1]
      let mut combined_inputs: Vec<Scalar> = (0..(consis_num_proofs - 1) * num_inputs).map(
        |i| (Scalar::one() - ry[0]) * exec_poly_io[i] + ry[0] * exec_poly_io[i + num_inputs]).collect();
      combined_inputs.extend((0..exec_inputs[0].len()).map(|i| (Scalar::one() - ry[0]) * exec_inputs[exec_inputs.len() - 1][i] + ry[0] * exec_inputs[0][i]));
      let combined_poly = DensePolynomial::new(combined_inputs);

      // let combined_poly = DensePolynomial::new(
      //   [(0..(consis_num_proofs - 1) * num_inputs).map(
      //     |i| (Scalar::one() - ry[0]) * exec_poly_io[i] + ry[0] * exec_poly_io[i + num_inputs]).collect(),
      //   vec![Scalar::zero(); exec_inputs[0].len()], exec_inputs[0].clone()].concat());

      let r = [rq.clone(), ry[1..].to_vec()].concat();
      
      let eval_vars_at_ry = combined_poly.evaluate(&r);
      
      let num_inputs: u64 = exec_inputs[0].len().try_into().unwrap();
      let r_plus_1: Vec<Scalar> = r.iter().map(|i| i + Scalar::from(num_inputs)).collect();
      let eval_vars_at_ry_plus_0 = exec_poly_io.evaluate(&r);
      let eval_vars_at_ry_plus_1 = exec_poly_io.evaluate(&r_plus_1);
      let eval_vars_at_ry_combined  = (Scalar::one() - ry[0]) * eval_vars_at_ry_plus_0 + ry[0] * eval_vars_at_ry_plus_1;

      println!("CLAIM: {:?}", Z_poly[0]);
      println!("EXPECTED 1: {:?}", eval_vars_at_ry);
      println!("EXPECTED 2: {:?}", eval_vars_at_ry_combined);

      let (proof_eval_vars_at_ry, comm_vars_at_ry) = PolyEvalProof::prove(
        &combined_poly,
        None,
        &r,
        &eval_vars_at_ry,
        None,
        &gens.gens_pc,
        transcript,
        random_tape,
      );

      /*
      let (proof_eval_vars_at_ry, comm_vars_at_ry) = PolyEvalProof::prove(
        // &combined_poly,
        & exec_poly_io,
        None,
        &r,
        &eval_vars_at_ry,
        None,
        &gens.gens_pc,
        transcript,
        random_tape,
      );

      let (proof_eval_vars_at_ry, comm_vars_at_ry) = PolyEvalProof::prove(
        // &combined_poly,
        & exec_poly_io,
        None,
        &r,
        &eval_vars_at_ry,
        None,
        &gens.gens_pc,
        transcript,
        random_tape,
      );
      */
      timer_polyeval.stop();
 
      // prove the final step of sum-check #2
      let blind_expected_claim_postsc2 = Scalar::zero();
      let claim_post_phase2 = claims_phase2[0] * claims_phase2[1];

      let (proof_eq_sc_phase2, _C1, _C2) = EqualityProof::prove(
        &gens.gens_pc.gens.gens_1,
        transcript,
        random_tape,
        &claim_post_phase2,
        &blind_expected_claim_postsc2,
        &claim_post_phase2,
        &blind_claim_postsc2,
      );

      timer_prove.stop();

      let claims_phase2 = (
        comm_Az_claim,
        comm_Bz_claim,
        comm_Cz_claim,
        comm_prod_Az_Bz_claims,
      );
      let pok_claims_phase2 = (
        pok_Cz_claim, proof_prod
      );

      (
        R1CSProofConsis {
          sc_proof_phase1,
          claims_phase2,
          pok_claims_phase2,
          proof_eq_sc_phase1,
          sc_proof_phase2,
          comm_vars_at_ry,
          proof_eval_vars_at_ry,
          proof_eq_sc_phase2
        },
        [rq, rx, ry]
      )
    };

    (
      R1CSProof {
        block_comm_vars_list,
        block_comm_io_list,
        exec_comm_io,
        block_proof,
        consis_proof
      },
      block_challenges,
      consis_challenges
    )
  }

  /*
  pub fn verify(
    &self,
    num_vars: usize,
    block_num_cons: usize,
    block_num_instances: usize,
    block_max_num_proofs: usize,
    block_num_proofs: &Vec<usize>,
    block_gens: &R1CSGens,
    block_evals: &(Scalar, Scalar, Scalar),
    consis_num_cons: usize,
    consis_num_proofs: usize,
    consis_gens: &R1CSGens,
    consis_evals: &(Scalar, Scalar, Scalar),
    transcript: &mut Transcript,
  ) -> Result<(Vec<Scalar>, Vec<Scalar>, Vec<Scalar>, Vec<Scalar>), ProofVerifyError> {
    transcript.append_protocol_name(R1CSProof::protocol_name());

    // --
    // COMMITMENTS
    // --

    // add the commitment to the verifier's transcript
    for p in 0..block_num_instances {
      self.block_comm_vars_list[p].append_to_transcript(b"poly_commitment", transcript);
      self.block_comm_io_list[p].append_to_transcript(b"poly_commitment", transcript);
    }
    self.exec_comm_io.append_to_transcript(b"poly_commitment", transcript);

    // --
    // BLOCK CORRECTNESS
    // --
    let (block_rp, block_rq_rev, block_rx, block_ry) = {
      let (num_rounds_x, num_rounds_p, num_rounds_q, num_rounds_y) = (block_num_cons.log_2(), block_num_instances.log_2(), block_max_num_proofs.log_2(), num_vars.log_2());

      let proof = self.block_proof;
      let gens = block_gens;

      // derive the verifier's challenge tau
      let tau_p = transcript.challenge_vector(b"challenge_tau_p", num_rounds_p);
      let tau_q = transcript.challenge_vector(b"challenge_tau_q", num_rounds_q);
      let tau_x = transcript.challenge_vector(b"challenge_tau_x", num_rounds_x);

      // verify the first sum-check instance
      let claim_phase1 = Scalar::zero()
        .commit(&Scalar::zero(), &gens.gens_sc.gens_1)
        .compress();
      let (comm_claim_post_phase1, rx) = proof.sc_proof_phase1.verify(
        &claim_phase1,
        num_rounds_x + num_rounds_q + num_rounds_p,
        3,
        &gens.gens_sc.gens_1,
        &gens.gens_sc.gens_4,
        transcript,
      )?;

      // perform the intermediate sum-check test with claimed Az, Bz, and Cz
      let (comm_Az_claim, comm_Bz_claim, comm_Cz_claim, comm_prod_Az_Bz_claims) = &proof.claims_phase2;
      let (pok_Cz_claim, proof_prod) = &proof.pok_claims_phase2;

      pok_Cz_claim.verify(&gens.gens_sc.gens_1, transcript, comm_Cz_claim)?;
      proof_prod.verify(
        &gens.gens_sc.gens_1,
        transcript,
        comm_Az_claim,
        comm_Bz_claim,
        comm_prod_Az_Bz_claims,
      )?;

      comm_Az_claim.append_to_transcript(b"comm_Az_claim", transcript);
      comm_Bz_claim.append_to_transcript(b"comm_Bz_claim", transcript);
      comm_Cz_claim.append_to_transcript(b"comm_Cz_claim", transcript);
      comm_prod_Az_Bz_claims.append_to_transcript(b"comm_prod_Az_Bz_claims", transcript);

      // Separate the result rx into rp_round1, rq, and rx
      let (rx, rq_rev) = rx.split_at(num_rounds_x);
      let (rq_rev, rp_round1) = rq_rev.split_at(num_rounds_q);
      let rx = rx.to_vec();
      let rq_rev = rq_rev.to_vec();
      let rq: Vec<Scalar> = rq_rev.iter().copied().rev().collect();
      let rp_round1 = rp_round1.to_vec();

      // taus_bound_rx is really taus_bound_rx_rq_rp
      let taus_bound_rp: Scalar = (0..rp_round1.len())
        .map(|i| rp_round1[i] * tau_p[i] + (Scalar::one() - rp_round1[i]) * (Scalar::one() - tau_p[i]))
        .product();
      let taus_bound_rq: Scalar = (0..rq_rev.len())
        .map(|i| rq_rev[i] * tau_q[i] + (Scalar::one() - rq_rev[i]) * (Scalar::one() - tau_q[i]))
        .product();
      let taus_bound_rx: Scalar = (0..rx.len())
        .map(|i| rx[i] * tau_x[i] + (Scalar::one() - rx[i]) * (Scalar::one() - tau_x[i]))
        .product();
      let taus_bound_rx = taus_bound_rp * taus_bound_rq * taus_bound_rx;

      let expected_claim_post_phase1 = (taus_bound_rx
        * (comm_prod_Az_Bz_claims.decompress().unwrap() - comm_Cz_claim.decompress().unwrap()))
      .compress();

      // verify proof that expected_claim_post_phase1 == claim_post_phase1
      proof.proof_eq_sc_phase1.verify(
        &gens.gens_sc.gens_1,
        transcript,
        &expected_claim_post_phase1,
        &comm_claim_post_phase1,
      )?;

      // derive three public challenges and then derive a joint claim
      let r_A = transcript.challenge_scalar(b"challenge_Az");
      let r_B = transcript.challenge_scalar(b"challenge_Bz");
      let r_C = transcript.challenge_scalar(b"challenge_Cz");

      // r_A * comm_Az_claim + r_B * comm_Bz_claim + r_C * comm_Cz_claim;
      let comm_claim_phase2 = GroupElement::vartime_multiscalar_mul(
        iter::once(&r_A)
          .chain(iter::once(&r_B))
          .chain(iter::once(&r_C)),
        iter::once(&comm_Az_claim)
          .chain(iter::once(&comm_Bz_claim))
          .chain(iter::once(&comm_Cz_claim))
          .map(|pt| pt.decompress().unwrap())
          .collect::<Vec<GroupElement>>(),
      )
      .compress();

      // verify the joint claim with a sum-check protocol
      let (comm_claim_post_phase2, ry) = proof.sc_proof_phase2.verify(
        &comm_claim_phase2,
        num_rounds_p + num_rounds_y,
        3,
        &gens.gens_sc.gens_1,
        &gens.gens_sc.gens_4,
        transcript,
      )?;

      // Separate ry into rp and ry
      let (rp, ry) = ry.split_at(num_rounds_p);
      let rp = rp.to_vec();
      let ry = ry.to_vec();

      // An Eq function to match p with rp
      let p_rp_poly_bound_ry: Scalar = (0..rp.len())
        .map(|i| rp[i] * rp_round1[i] + (Scalar::one() - rp[i]) * (Scalar::one() - rp_round1[i]))
        .product();

      // verify Z(rp, rq, ry) proof against the initial commitment
      // First instance-by-instance on ry
      for p in 0..block_num_instances {
        // comm_combined = (Scalar::one() - ry[0]) * block_comm_vars + ry[0] * comm_inputs
        let comm_combined = PolyCommitment {
          C: (0..self.block_comm_vars_list[p].C.len()).map(
            |i| ((Scalar::one() - ry[0]) * self.block_comm_vars_list[p].C[i].decompress().unwrap()
                        + ry[0] * self.block_comm_io_list[p].C[i].decompress().unwrap()).compress()
          ).collect()
        };

        // if num_proofs[p] < max_num_proofs, then only the last few entries of rq needs to be binded
        let rq_short = &rq[num_rounds_q - block_num_proofs[p].log_2()..];
        let r = [rq_short, &ry[1..]].concat();

        proof.proof_eval_vars_at_ry_list[p].verify(
          &gens.gens_pc,
          transcript,
          &r,
          &proof.comm_vars_at_ry_list[p],
          &comm_combined,
        )?;
      }

      // Then on rp
      let mut expected_block_comm_vars_list: Vec<RistrettoPoint> = proof.comm_vars_at_ry_list.iter().map(|i| i.decompress().unwrap()).collect();
      for p in 0..block_num_instances {
        for q in 0..(num_rounds_q - block_num_proofs[p].log_2()) {
          expected_block_comm_vars_list[p] *= Scalar::one() - rq[q];
        }
      }
      let EQ_p = EqPolynomial::new(rp.clone()).evals();
      let expected_comm_vars_at_ry = GroupElement::vartime_multiscalar_mul(&EQ_p, expected_block_comm_vars_list).compress();
      assert_eq!(expected_comm_vars_at_ry, proof.comm_vars_at_ry);

      // compute commitment to eval_Z_at_ry = (Scalar::one() - ry[0]) * self.eval_vars_at_ry + ry[0] * poly_input_eval
      let comm_eval_Z_at_ry = &proof.comm_vars_at_ry.decompress().unwrap();

      // perform the final check in the second sum-check protocol
      let (eval_A_r, eval_B_r, eval_C_r) = block_evals;
      let expected_claim_post_phase2 =
        ((r_A * eval_A_r + r_B * eval_B_r + r_C * eval_C_r) * comm_eval_Z_at_ry * p_rp_poly_bound_ry).compress();
      // verify proof that expected_claim_post_phase2 == claim_post_phase2
      proof.proof_eq_sc_phase2.verify(
        &gens.gens_sc.gens_1,
        transcript,
        &expected_claim_post_phase2,
        &comm_claim_post_phase2,
      )?;
      (rp, rq_rev, rx, ry)
    };

    // --
    // CONSISTENCY
    // --
    let (consis_rq, consis_rx, consis_ry) = {
      let (num_rounds_q, num_rounds_x, num_rounds_y) = (consis_num_proofs.log_2(), consis_num_cons.log_2(), num_vars.log_2());

      let proof = self.consis_proof;
      let gens = consis_gens;

      // derive the verifier's challenge tau
      let tau = transcript.challenge_vector(b"challenge_tau", num_rounds_q + num_rounds_x);

      // verify the first sum-check instance
      let claim_phase1 = Scalar::zero()
        .commit(&Scalar::zero(), &consis_gens.gens_sc.gens_1)
        .compress();
      let (comm_claim_post_phase1, rx) = proof.sc_proof_phase1.verify(
        &claim_phase1,
        num_rounds_q + num_rounds_x,
        3,
        &gens.gens_sc.gens_1,
        &gens.gens_sc.gens_4,
        transcript,
      )?;

      // perform the intermediate sum-check test with claimed Az, Bz, and Cz
      let (comm_Az_claim, comm_Bz_claim, comm_Cz_claim, comm_prod_Az_Bz_claims) = &proof.claims_phase2;
      let (pok_Cz_claim, proof_prod) = &proof.pok_claims_phase2;

      pok_Cz_claim.verify(&gens.gens_sc.gens_1, transcript, comm_Cz_claim)?;
      proof_prod.verify(
        &gens.gens_sc.gens_1,
        transcript,
        comm_Az_claim,
        comm_Bz_claim,
        comm_prod_Az_Bz_claims,
      )?;

      comm_Az_claim.append_to_transcript(b"comm_Az_claim", transcript);
      comm_Bz_claim.append_to_transcript(b"comm_Bz_claim", transcript);
      comm_Cz_claim.append_to_transcript(b"comm_Cz_claim", transcript);
      comm_prod_Az_Bz_claims.append_to_transcript(b"comm_prod_Az_Bz_claims", transcript);

      // taus_bound_rx is really taus_bound_rq_rx
      let taus_bound_rx: Scalar = (0..rx.len())
        .map(|i| rx[i] * tau[i] + (Scalar::one() - rx[i]) * (Scalar::one() - tau[i]))
        .product();

      let expected_claim_post_phase1 = (taus_bound_rx
        * (comm_prod_Az_Bz_claims.decompress().unwrap() - comm_Cz_claim.decompress().unwrap()))
      .compress();

      // Separate the result rx into rp_round1, rq, and rx
      let (rq, rx) = rx.split_at(num_rounds_q);
      let rq = rq.to_vec();
      let rx = rx.to_vec();

      // verify proof that expected_claim_post_phase1 == claim_post_phase1
      proof.proof_eq_sc_phase1.verify(
        &gens.gens_sc.gens_1,
        transcript,
        &expected_claim_post_phase1,
        &comm_claim_post_phase1,
      )?;

      // derive three public challenges and then derive a joint claim
      let r_A = transcript.challenge_scalar(b"challenge_Az");
      let r_B = transcript.challenge_scalar(b"challenge_Bz");
      let r_C = transcript.challenge_scalar(b"challenge_Cz");

      // r_A * comm_Az_claim + r_B * comm_Bz_claim + r_C * comm_Cz_claim;
      let comm_claim_phase2 = GroupElement::vartime_multiscalar_mul(
        iter::once(&r_A)
          .chain(iter::once(&r_B))
          .chain(iter::once(&r_C)),
        iter::once(&comm_Az_claim)
          .chain(iter::once(&comm_Bz_claim))
          .chain(iter::once(&comm_Cz_claim))
          .map(|pt| pt.decompress().unwrap())
          .collect::<Vec<GroupElement>>(),
      )
      .compress();

      // verify the joint claim with a sum-check protocol
      let (comm_claim_post_phase2, ry) = proof.sc_proof_phase2.verify(
        &comm_claim_phase2,
        num_rounds_y,
        2,
        &gens.gens_sc.gens_1,
        &gens.gens_sc.gens_3,
        transcript,
      )?;

      // verify Z(rq, ry) proof against the initial commitment
      // Compute combined_poly as (Scalar::one() - ry[0]) * exec_poly_io[i] + ry[0] * exec_poly_io[i + 1]
      let combined_poly = DensePolynomial::new(
        (0..consis_num_proofs * num_inputs).map(
          |i| (Scalar::one() - ry[0]) * exec_poly_io[i] + ry[0] * exec_poly_io[i + num_inputs]).collect());


      // First instance-by-instance on ry
      // comm_combined = (Scalar::one() - ry[0]) * block_comm_vars + ry[0] * comm_inputs
      let comm_combined = PolyCommitment {
        C: (0..self.block_comm_vars_list[p].C.len()).map(
          |i| ((Scalar::one() - ry[0]) * self.block_comm_vars_list[p].C[i].decompress().unwrap()
                      + ry[0] * self.block_comm_io_list[p].C[i].decompress().unwrap()).compress()
        ).collect()
      };

      // if num_proofs[p] < max_num_proofs, then only the last few entries of rq needs to be binded
      let rq_short = &rq[num_rounds_q - block_num_proofs[p].log_2()..];
      let r = [rq_short, &ry[1..]].concat();

      proof.proof_eval_vars_at_ry_list[p].verify(
        &gens.gens_pc,
        transcript,
        &r,
        &proof.comm_vars_at_ry_list[p],
        &comm_combined,
      )?;

      // Then on rp
      let mut expected_block_comm_vars_list: Vec<RistrettoPoint> = proof.comm_vars_at_ry_list.iter().map(|i| i.decompress().unwrap()).collect();
      for p in 0..block_num_instances {
        for q in 0..(num_rounds_q - block_num_proofs[p].log_2()) {
          expected_block_comm_vars_list[p] *= Scalar::one() - rq[q];
        }
      }
      let EQ_p = EqPolynomial::new(rp.clone()).evals();
      let expected_comm_vars_at_ry = GroupElement::vartime_multiscalar_mul(&EQ_p, expected_block_comm_vars_list).compress();
      assert_eq!(expected_comm_vars_at_ry, proof.comm_vars_at_ry);

      // compute commitment to eval_Z_at_ry = (Scalar::one() - ry[0]) * self.eval_vars_at_ry + ry[0] * poly_input_eval
      let comm_eval_Z_at_ry = &proof.comm_vars_at_ry.decompress().unwrap();

      // perform the final check in the second sum-check protocol
      let (eval_A_r, eval_B_r, eval_C_r) = block_evals;
      let expected_claim_post_phase2 =
        ((r_A * eval_A_r + r_B * eval_B_r + r_C * eval_C_r) * comm_eval_Z_at_ry * p_rp_poly_bound_ry).compress();
      // verify proof that expected_claim_post_phase2 == claim_post_phase2
      proof.proof_eq_sc_phase2.verify(
        &gens.gens_sc.gens_1,
        transcript,
        &expected_claim_post_phase2,
        &comm_claim_post_phase2,
      )?;
      (rq, rx, ry)
    };

    Ok((block_rp, block_rq_rev, block_rx, block_ry))
  }
  */
}

#[cfg(test)]
mod tests {
  use super::*;
  use rand::rngs::OsRng;

  fn produce_tiny_r1cs() -> (R1CSInstance, Vec<Scalar>, Vec<Scalar>) {
    // three constraints over five variables Z1, Z2, Z3, Z4, and Z5
    // rounded to the nearest power of two
    let num_cons = 128;
    let num_vars = 256;
    let num_inputs = 2;

    // encode the above constraints into three matrices
    let mut A: Vec<(usize, usize, Scalar)> = Vec::new();
    let mut B: Vec<(usize, usize, Scalar)> = Vec::new();
    let mut C: Vec<(usize, usize, Scalar)> = Vec::new();

    let one = Scalar::one();
    // constraint 0 entries
    // (Z1 + Z2) * I0 - Z3 = 0;
    A.push((0, 0, one));
    A.push((0, 1, one));
    B.push((0, num_vars + 1, one));
    C.push((0, 2, one));

    // constraint 1 entries
    // (Z1 + I1) * (Z3) - Z4 = 0
    A.push((1, 0, one));
    A.push((1, num_vars + 2, one));
    B.push((1, 2, one));
    C.push((1, 3, one));
    // constraint 3 entries
    // Z5 * 1 - 0 = 0
    A.push((2, 4, one));
    B.push((2, num_vars, one));

    let inst = R1CSInstance::new(num_cons, num_vars, num_inputs, &A, &B, &C);

    // compute a satisfying assignment
    let mut csprng: OsRng = OsRng;
    let i0 = Scalar::random(&mut csprng);
    let i1 = Scalar::random(&mut csprng);
    let z1 = Scalar::random(&mut csprng);
    let z2 = Scalar::random(&mut csprng);
    let z3 = (z1 + z2) * i0; // constraint 1: (Z1 + Z2) * I0 - Z3 = 0;
    let z4 = (z1 + i1) * z3; // constraint 2: (Z1 + I1) * (Z3) - Z4 = 0
    let z5 = Scalar::zero(); //constraint 3

    let mut vars = vec![Scalar::zero(); num_vars];
    vars[0] = z1;
    vars[1] = z2;
    vars[2] = z3;
    vars[3] = z4;
    vars[4] = z5;

    let mut input = vec![Scalar::zero(); num_inputs];
    input[0] = i0;
    input[1] = i1;

    (inst, vars, input)
  }

  #[test]
  fn test_tiny_r1cs() {
    let (inst, vars, input) = tests::produce_tiny_r1cs();
    let is_sat = inst.is_sat(&vars, &input);
    assert!(is_sat);
  }

  #[test]
  fn test_synthetic_r1cs() {
    let (inst, vars, input) = R1CSInstance::produce_synthetic_r1cs(1024, 1024, 10);
    let is_sat = inst.is_sat(&vars, &input);
    assert!(is_sat);
  }

  #[test]
  pub fn check_r1cs_proof() {
    let num_vars = 1024;
    let num_cons = num_vars;
    let num_inputs = 10;
    let (inst, vars, input) = R1CSInstance::produce_synthetic_r1cs(num_cons, num_vars, num_inputs);

    let gens = R1CSGens::new(b"test-m", num_cons, num_vars);

    let mut random_tape = RandomTape::new(b"proof");
    let mut prover_transcript = Transcript::new(b"example");
    let (proof, rx, ry) = R1CSProof::prove(
      &inst,
      vars,
      &input,
      &gens,
      &mut prover_transcript,
      &mut random_tape,
    );

    let inst_evals = inst.evaluate(&rx, &ry);

    let mut verifier_transcript = Transcript::new(b"example");
    assert!(proof
      .verify(
        inst.get_num_vars(),
        inst.get_num_cons(),
        &input,
        &inst_evals,
        &mut verifier_transcript,
        &gens,
      )
      .is_ok());
  }
}
