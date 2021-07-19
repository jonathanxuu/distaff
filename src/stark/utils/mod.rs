use rand::prelude::*;
use rand::distributions::Uniform;
use rand::{Rng};
// use rand_chacha::rand_core::SeedableRng;
// use rand_chacha::rand_core;
// use crate::stark::trace::trace_state::fmt::string::lossy::char::methods::unicode::N;

use super::{ ProofOptions, MAX_CONSTRAINT_DEGREE };
use sp_std::vec::Vec;
use wasm_bindgen_test::console_log;

// RE-EXPORTS
// ================================================================================================
mod coefficients;
pub use coefficients::{ ConstraintCoefficients, CompositionCoefficients };

mod proof_of_work;
pub use proof_of_work::{ find_pow_nonce, verify_pow_nonce };

pub fn get_composition_degree(trace_length: usize) -> usize {
    return (MAX_CONSTRAINT_DEGREE - 1) * trace_length - 1;
}

// PUBLIC FUNCTIONS
// ================================================================================================

pub fn get_incremental_trace_degree(trace_length: usize) -> usize {
    let composition_degree = get_composition_degree(trace_length);
    return composition_degree - (trace_length - 2);
}

pub fn compute_query_positions(seed: &[u8; 32], domain_size: usize, options: &ProofOptions) -> Vec<usize> {
    let range = Uniform::from(0..domain_size);
    log::debug!(target:"starks-verifier","seeeeed is {:?}",seed);
    println!("seeed is {:?},domainsize is {:?}",seed,domain_size);
    console_log!("seeed is {:?},domainsize is {:?}",seed,domain_size);

    let mut index_iter = StdRng::from_seed(*seed).sample_iter(range);
    let num_queries = options.num_queries();

    let mut result = Vec::new();
    log::debug!(target:"starks-verifier","range is {:?},index_iter is {:?},num_queries is {:?}.result is {:?}",range,index_iter,num_queries,result);

    for _ in 0..1000 {
        let value = index_iter.next().unwrap();
        // console_log!("value is {:?}",value);

        if value % options.extension_factor() == 0 { continue; }
        // console_log!("value is {:?},options .extension is {:?}",value, options.extension_factor());

        if result.contains(&value) { continue; }
        result.push(value);
        if result.len() >= num_queries { break; }
    }
    // console_log!("result after for1000 is {:?},len is {:?}",result,result.len());
    log::debug!(target:"starks-verifier","result after for1000 is {:?},len is {:?}",result,result.len());
    if result.len() < num_queries {
        panic!("needed to generate {} query positions, but generated only {}", num_queries, result.len());
    }

    return result;
}

pub fn map_trace_to_constraint_positions(positions: &[usize]) -> Vec<usize> {
    let mut result = Vec::with_capacity(positions.len());
    for &position in positions.iter() {
        let cp = position / 2;
        if !result.contains(&cp) { result.push(cp); }
    }
    return result;
}