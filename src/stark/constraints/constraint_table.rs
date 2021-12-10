use crate::math::{ field, parallel, fft, polynom };
use crate::stark::{ TraceTable, TraceState };
use crate::utils::{ uninit_vector };
use super::{ ConstraintEvaluator, ConstraintPoly };
use wasm_bindgen_test::*;
use sp_std::{vec, vec::Vec};

// TYPES AND INTERFACES
// ================================================================================================
#[derive(Debug)]
pub struct ConstraintTable {
    evaluator       : ConstraintEvaluator,
    i_evaluations   : Vec<u128>,    // combined evaluations of boundary constraints at the first step
    f_evaluations   : Vec<u128>,    // combined evaluations of boundary constraints at the last step
    t_evaluations   : Vec<u128>,    // combined evaluations of transition constraints
}

// CONSTRAINT TABLE IMPLEMENTATION
// ================================================================================================
impl ConstraintTable {
    pub fn new(trace: &TraceTable, trace_root: &[u8; 32], inputs: &[u128], outputs: &[u128]) -> ConstraintTable {
        // 这里传入的参数： trace的register是extend之后的，包含25个8192个点值的数组，trace_root是这些点值构成的roothash
        // input是public_input（18），output是1——————换句话说，也就是stack：trace register最初的状态和最终的状态
        
        let evaluator = ConstraintEvaluator::from_trace(trace, trace_root, inputs, outputs);
        let evaluation_domain_size = evaluator.domain_size(); // 2048
        return ConstraintTable {
            evaluator       : evaluator,
                // return Evaluator {
                // decoder         : decoder,
                // stack           : stack,
                // coefficients    : ConstraintCoefficients::new(*trace_root, ctx_depth, loop_depth, stack_depth),// 返回的是两个 boundary_coefficients 和一个拥有68个元素的数组
                // domain_size     : domain_size, //2048    这里是|d_ev|的domain size， 比 trace_domain 大了 8 倍 
                // extension_factor: extension_factor, // 8
                // t_constraint_num: t_constraint_degrees.len(), // 34
                // t_degree_groups : group_transition_constraints(t_constraint_degrees, trace_length), //34个元素, 256
                // t_evaluations   : t_evaluations, //空
                // b_constraint_num: get_boundary_constraint_num(&inputs, &outputs), //1 + 2 + inputs长度 + outputs长度
                // program_hash    : last_state.program_hash().to_vec(), 
                // op_count        : last_state.op_counter(),
                // inputs          : inputs.to_vec(),
                // outputs         : outputs.to_vec(),
                // b_degree_adj    : get_boundary_constraint_adjustment_degree(trace_length), // 约束 乘以 这个 度调整因子 ，会变成度都为|D_ev|- |D_trace|的多项式
            i_evaluations   : uninit_vector(evaluation_domain_size),// 2048 个空
            f_evaluations   : uninit_vector(evaluation_domain_size),// 2048 个空
            t_evaluations   : uninit_vector(evaluation_domain_size),// 2048 个空
        };
    }

    /// Returns the total number of transition and boundary constraints.
    pub fn constraint_count(&self) -> usize {
        return self.evaluator.constraint_count();
    }

    /// Returns the size of the evaluation domain = trace_length * MAX_CONSTRAINT_DEGREE
    pub fn evaluation_domain_size(&self) -> usize {
        return self.evaluator.domain_size();
    }

    /// Returns the length of the un-extended execution trace.
    pub fn trace_length(&self) -> usize {
        return self.evaluator.trace_length();
    }

    /// Evaluates transition and boundary constraints at the specified step.
    /// 在当前step计算 状态转移和边界约束
    /// 调用函数为：  constraints.evaluate(&current, &next, lde_domain[i], i / stride);
    pub fn evaluate(&mut self, current: &TraceState, next: &TraceState, x: u128, step: usize) {
        
        // 首先 利用 current状态 和 lde的某个点，计算init_bound 和 last_bound
        let (init_bound, last_bound) = self.evaluator.evaluate_boundaries(current, x);

        self.i_evaluations[step] = init_bound;
        self.f_evaluations[step] = last_bound;
        console_log!(" i_evaluations and f_evaluations are {:?}, {:?}",init_bound,last_bound);
        // 然后是边界约束的 evaluation
        self.t_evaluations[step] = self.evaluator.evaluate_transition(current, next, x, step);
    }

    /// Interpolates all constraint evaluations into polynomials and combines all these 
    /// polynomials into a single polynomial using pseudo-random linear combination.
    pub fn combine_polys(mut self) -> ConstraintPoly
    {
        let combination_root = field::get_root_of_unity(self.evaluation_domain_size());
        let inv_twiddles = fft::get_inv_twiddles(combination_root, self.evaluation_domain_size());
     
        #[cfg(debug_assertions)]
        self.validate_transition_degrees();
        
        let mut combined_poly = uninit_vector(self.evaluation_domain_size());
        
        // 1 ----- boundary constraints for the initial step --------------------------------------
        // interpolate initial step boundary constraint combination into a polynomial, divide the 
        // polynomial by Z(x) = (x - 1), and add it to the result
        polynom::interpolate_fft_twiddles(&mut self.i_evaluations, &inv_twiddles, true);
        polynom::syn_div_in_place(&mut self.i_evaluations, field::ONE);
        combined_poly.copy_from_slice(&self.i_evaluations);

        // 2 ----- boundary constraints for the final step ----------------------------------------
        // interpolate final step boundary constraint combination into a polynomial, divide the 
        // polynomial by Z(x) = (x - x_at_last_step), and add it to the result
        polynom::interpolate_fft_twiddles(&mut self.f_evaluations, &inv_twiddles, true);
        let x_at_last_step = self.evaluator.get_x_at_last_step();
        polynom::syn_div_in_place(&mut self.f_evaluations, x_at_last_step);
        parallel::add_in_place(&mut combined_poly, &self.f_evaluations, 1);

        // 3 ----- transition constraints ---------------------------------------------------------
        // interpolate transition constraint combination into a polynomial, divide the polynomial
        // by Z(x) = (x^steps - 1) / (x - x_at_last_step), and add it to the result
        let trace_length = self.trace_length();
        polynom::interpolate_fft_twiddles(&mut self.t_evaluations, &inv_twiddles, true);
        polynom::syn_div_expanded_in_place(&mut self.t_evaluations, trace_length, &[x_at_last_step]);
        parallel::add_in_place(&mut combined_poly, &self.t_evaluations, 1);

        return ConstraintPoly::new(combined_poly);
    }

    #[cfg(debug_assertions)]
    fn validate_transition_degrees(&self) {
        let trace_degree = self.evaluator.trace_length() - 1;
        let mut expected_degrees = self.evaluator.get_transition_degrees();
        for i in 0..expected_degrees.len() {
            expected_degrees[i] = expected_degrees[i] * trace_degree;
        }

        let mut actual_degrees = Vec::new();
        let transition_evaluations = self.evaluator.get_transition_evaluations();
        for i in 0..transition_evaluations.len() {
            let degree = crate::math::polynom::infer_degree(&transition_evaluations[i]);
            actual_degrees.push(degree);
        }

        for i in 0..expected_degrees.len() {
            if expected_degrees[i] < actual_degrees[i] {
                panic!("constraint degrees didn't match\nexpected: {:>3?}\nactual:   {:>3?}",
                    expected_degrees, actual_degrees);
                   
            }
        }
    }
}