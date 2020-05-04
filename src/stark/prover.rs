use std::time::Instant;
use log::debug;
use std::collections::BTreeMap;
use crate::math::{ field, polynom, parallel };
use crate::utils::{ CopyInto };

use super::trace::{ TraceTable, TraceState };
use super::constraints::{ ConstraintEvaluator, ConstraintTable, MAX_CONSTRAINT_DEGREE };
use super::{ ProofOptions, StarkProof, fri, utils::QueryIndexGenerator, CompositionCoefficients, DeepValues };

// PROVER FUNCTION
// ================================================================================================

pub fn prove(trace: &mut TraceTable, program_hash: &[u64; 4], inputs: &[u64], outputs: &[u64], options: &ProofOptions) -> StarkProof {

    // 1 ----- extend execution trace -------------------------------------------------------------
    let now = Instant::now();
    trace.extend();
    debug!("Extended execution trace of {} registers to {} steps in {} ms",
        trace.register_count(),
        trace.domain_size(), 
        now.elapsed().as_millis());

    // 2 ----- build Merkle tree from extended execution trace ------------------------------------
    let now = Instant::now();
    let trace_tree = trace.to_merkle_tree(options.hash_function());
    debug!("Built trace Merkle tree in {} ms", 
        now.elapsed().as_millis());

    // 3 ----- build evaluation domain for the extended execution trace ---------------------------
    let now = Instant::now();
    let root = field::get_root_of_unity(trace.domain_size() as u64);
    let domain = field::get_power_series(root, trace.domain_size());
    debug!("Built evaluation domain of {} elements in {} ms",
        domain.len(),
        now.elapsed().as_millis());

    // 4 ----- evaluate constraints ---------------------------------------------------------------
    let now = Instant::now();
    
    // initialize constraint evaluator
    let constraint_evaluator = ConstraintEvaluator::new(
        trace_tree.root(), 
        trace.unextended_length(),
        trace.max_stack_depth(),
        MAX_CONSTRAINT_DEGREE,
        program_hash,
        inputs,
        outputs);

    // allocate space to hold constraint evaluations
    let mut constraints = ConstraintTable::new(constraint_evaluator, domain);
    
    // allocate space to hold current and next states for constraint evaluations
    let mut current = TraceState::new(trace.max_stack_depth());
    let mut next = TraceState::new(trace.max_stack_depth());

    // we don't need to evaluate constraints over the entire extended execution trace; we need
    // to evaluate them over the domain extended to match max constraint degree - thus, we can
    // skip most trace states for the purposes of constraint evaluation.
    for i in (0..trace.domain_size()).step_by(constraints.domain_stride()) {
        // TODO: this loop should be parallelized and also potentially optimized to avoid copying
        // next state from the trace table twice

        // copy current and next states from the trace table; next state may wrap around the
        // execution trace (close to the end of the trace)
        trace.fill_state(&mut current, i);
        trace.fill_state(&mut next, (i + trace.extension_factor()) % trace.domain_size());

        // evaluate the constraints
        constraints.evaluate(&current, &next, i);
    }

    debug!("Evaluated {} constraints in {} ms",
        constraints.constraint_count(),
        now.elapsed().as_millis());

    // 5 ----- convert constraint evaluations into a polynomial -----------------------------------
    let now = Instant::now();
    let constraint_poly = constraints.into_combination_poly();
    debug!("Converted constraint evaluations into a single polynomial of degree {} in {} ms",
        constraint_poly.degree(),
        now.elapsed().as_millis());

    // 6 ----- build Merkle tree from constraint polynomial evaluations ---------------------------
    let now = Instant::now();
    let constraint_tree = constraint_poly.to_merkle_tree(options.hash_function());
    debug!("Evaluated constraint polynomial and built constraint Merkle tree in {} ms",
        now.elapsed().as_millis());

    // 7 ----- build and evaluate deep composition polynomial -------------------------------------
    let now = Instant::now();

    // use constraint tree root to determine deep point z and coefficients for random linear
    // combinations used to build deep composition polynomial
    let z = field::prng(constraint_tree.root().copy_into());
    let deep_values = DeepValues::new(z, &trace, &constraint_poly);
    let coefficients = CompositionCoefficients::new(constraint_tree.root());

    // build constraint and trace composition polynomials and add them together
    let composition_degree = constraint_poly.degree() - 1;
    let t_composition_poly = trace.get_composition_poly(deep_values.z, composition_degree, &coefficients); // TODO
    let c_composition_poly = constraint_poly.get_composition_poly(deep_values.z, &coefficients); // TODO
    let composition_poly = parallel::add(&t_composition_poly, &c_composition_poly, 1);

    // evaluate the composition polynomial
    let mut composed_evaluations = composition_poly;
    composed_evaluations.resize(trace.domain_size(), 0);
    polynom::eval_fft(&mut composed_evaluations, true);  // TODO: use twiddles

    debug!("Built composition polynomial and evaluated it over domain of {} elements in {} ms",
        composed_evaluations.len(),
        now.elapsed().as_millis());

    // 8 ----- generate low-degree proof for composition polynomial -------------------------------
    let now = Instant::now();
    let composition_degree_plus_1 = composed_evaluations.len() - trace.unextended_length(); // TODO: compute correctly
    let fri_proof = fri::prove(
        &composed_evaluations,
        constraint_poly.domain(),
        composition_degree_plus_1,
        options);
    debug!("Generated low-degree proof for composition polynomial in {} ms",
        now.elapsed().as_millis());

    // 9 ----- query extended execution trace at pseudo-random positions --------------------------
    let now = Instant::now();

    // generate pseudo-random indexes based on the root of the composition Merkle tree
    let idx_generator = QueryIndexGenerator::new(options);
    let positions = idx_generator.get_trace_indexes(&fri_proof.ev_root, trace.domain_size());

    // for each queried step, collect the current and the next states of the execution trace;
    // this way, the verifier will be able to get two consecutive states for each query.
    let mut trace_states = BTreeMap::new();
    for &position in positions.iter() {
        let next_position = (position + options.extension_factor()) % trace.domain_size();

        trace_states.insert(position, trace.get_state(position));
        trace_states.insert(next_position, trace.get_state(next_position));
    }

    // sort the positions and corresponding states so that their orders align
    let augmented_positions = trace_states.keys().cloned().collect::<Vec<usize>>();
    let trace_states = trace_states.into_iter().map(|(_, v)| v).collect();

    // build the proof object
    let proof = StarkProof::new(
        trace_tree.root(),
        trace_tree.prove_batch(&augmented_positions),
        trace_states,
        constraint_tree.root(),
        constraint_tree.prove_batch(&[1, 2, 3, 4]), // TODO
        deep_values,
        fri_proof,
        &options);

    debug!("Computed {} trace queries and built proof object in {} ms",
        positions.len(),
        now.elapsed().as_millis());

    return proof;
}