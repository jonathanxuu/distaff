use crate::{PROGRAM_DIGEST_SIZE, math::field, stark::{ConstraintCoefficients, StarkProof, TraceState, TraceTable, constraints::stack}, utils::uninit_vector};
use super::{ decoder::Decoder, stack::Stack, super::MAX_CONSTRAINT_DEGREE };
use sp_std::{vec, vec::Vec};
use wasm_bindgen_test::console_log;
// TYPES AND INTERFACES
// ================================================================================================
#[derive(Debug)]
pub struct Evaluator {
    decoder         : Decoder,
    stack           : Stack,

    coefficients    : ConstraintCoefficients,
    domain_size     : usize,
    extension_factor: usize,

    t_constraint_num: usize,
    t_degree_groups : Vec<(u128, Vec<usize>)>,
    t_evaluations   : Vec<Vec<u128>>,

    b_constraint_num: usize,
    program_hash    : Vec<u128>,
    op_count        : u128,
    inputs          : Vec<u128>,
    outputs         : Vec<u128>,
    b_degree_adj    : u128,
}

// EVALUATOR IMPLEMENTATION
// ================================================================================================
impl Evaluator {

    pub fn from_trace(trace: &TraceTable, trace_root: &[u8; 32], inputs: &[u128], outputs: &[u128]) -> Evaluator
    {       
        // 这里传入的参数： trace的register是extend之后的，包含25个8192个点值的数组，trace_root是这些点值的roothash，
        // input是public_input（18），output是1———— 换句话说，也就是stack：trace register 最初值和最终值

        let last_state = trace.get_last_state(); // 这里get到的虽然是扩展之后的 8160步的 ，但是获得的和lib.rs里面的last_state仍然是同一个
        let ctx_depth = trace.ctx_depth(); // 0
        let loop_depth = trace.loop_depth();// 0
        let stack_depth = trace.stack_depth();// 10
        let trace_length = trace.unextended_length(); // 256
        let extension_factor = MAX_CONSTRAINT_DEGREE; // 8 默认值 , 这里和src/stack/readme.md里的 第二个domian相对应，这个domain大小是trace domain的 8 倍

        // instantiate decoder and stack constraint evaluators 
        let decoder = Decoder::new(trace_length, extension_factor, ctx_depth, loop_depth);
        let stack = Stack::new(trace_length, extension_factor, stack_depth);

        // 将两个degrees收尾拼接，22位[2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 8, 8, 6, 4, 6, 7, 6, 6, 4, 4, 4] 和12个7
        // 拼接结果： 共34位⬆️
        // build a list of transition constraint degrees
        let t_constraint_degrees = [
            decoder.constraint_degrees(), stack.constraint_degrees()
        ].concat();


        // if we are in debug mode, initialize vectors to hold individual evaluations
        // of transition constraints
        // domain_size = 256 * 8 = 2048
        let domain_size = trace_length * extension_factor;
        let t_evaluations = if cfg!(debug_assertions) {
            t_constraint_degrees.iter().map(|_| uninit_vector(domain_size)).collect()
        }
        else {
            Vec::new()
        };
        
        return Evaluator {
            decoder         : decoder,
            stack           : stack,
            coefficients    : ConstraintCoefficients::new(*trace_root, ctx_depth, loop_depth, stack_depth),// 返回的是两个 boundary_coefficients 和一个拥有68个元素的数组
            domain_size     : domain_size, //2048 这里是|d_ev|的domain size， 比 trace_domain 大了 8 倍 
            extension_factor: extension_factor, // 8
            t_constraint_num: t_constraint_degrees.len(), // 34
            t_degree_groups : group_transition_constraints(t_constraint_degrees, trace_length), //34个元素, 256
            t_evaluations   : t_evaluations, //空
            b_constraint_num: get_boundary_constraint_num(&inputs, &outputs), //1 + 2 + inputs长度 + outputs长度 等于 1+ 8 = 12
            program_hash    : last_state.program_hash().to_vec(), 
            op_count        : last_state.op_counter(),
            inputs          : inputs.to_vec(),
            outputs         : outputs.to_vec(),
            b_degree_adj    : get_boundary_constraint_adjustment_degree(trace_length), // 约束 乘以 这个 度调整因子 ，会变成度都为|D_ev|- |D_trace|的多项式
        };
    }

    pub fn from_proof(proof: &StarkProof, program_hash: &[u8; 32], inputs: &[u128], outputs: &[u128]) -> Evaluator
    {
        let ctx_depth = proof.ctx_depth();
        let loop_depth = proof.loop_depth();
        let stack_depth = proof.stack_depth();
        let trace_length = proof.trace_length();
        let extension_factor = proof.options().extension_factor();
        
        // instantiate decoder and stack constraint evaluators 
        let decoder = Decoder::new(trace_length, extension_factor, ctx_depth, loop_depth);
        let stack = Stack::new(trace_length, extension_factor, stack_depth);

        // build a list of transition constraint degrees
        let t_constraint_degrees = [
            decoder.constraint_degrees(), stack.constraint_degrees()
        ].concat();

        return Evaluator {
            decoder         : decoder,
            stack           : stack,
            coefficients    : ConstraintCoefficients::new(*proof.trace_root(), ctx_depth, loop_depth, stack_depth),
            domain_size     : proof.domain_size(),
            extension_factor: extension_factor,
            t_constraint_num: t_constraint_degrees.len(),
            t_degree_groups : group_transition_constraints(t_constraint_degrees, trace_length),
            t_evaluations   : Vec::new(),
            b_constraint_num: get_boundary_constraint_num(&inputs, &outputs),
            program_hash    : parse_program_hash(program_hash),
            op_count        : proof.op_count(),
            inputs          : inputs.to_vec(),
            outputs         : outputs.to_vec(),
            b_degree_adj    : get_boundary_constraint_adjustment_degree(trace_length),
        };
    }

    pub fn constraint_count(&self) -> usize {
        return self.t_constraint_num + self.b_constraint_num;
    }

    pub fn domain_size(&self) -> usize {
        return self.domain_size;
    }

    pub fn trace_length(&self) -> usize {
        return self.domain_size / self.extension_factor;
    }

    pub fn get_x_at_last_step(&self) -> u128 {
        let trace_root = field::get_root_of_unity(self.trace_length());
        return field::exp(trace_root, (self.trace_length() - 1) as u128);
    }

    // CONSTRAINT EVALUATORS
    // -------------------------------------------------------------------------------------------

    /// Computes pseudo-random linear combination of transition constraints D_i at point x as:
    /// cc_{i * 2} * D_i + cc_{i * 2 + 1} * D_i * x^p for all i, where cc_j are the coefficients
    /// used in the linear combination and x^p is a degree adjustment factor (different for each degree).
    pub fn evaluate_transition(&self, current: &TraceState, next: &TraceState, x: u128, step: usize) -> u128 {
        // 边界约束，一旦step设置的不对，会导致错误，为什么呢？
        // current 一开始是 op_counter=0, next 是 op_counter=1，也就是一个完美的transition

        // evaluate transition constraints
        // 设置 34 个 0 的 evaluations 数组
        let mut evaluations = vec![field::ZERO; self.t_constraint_num];   

        self.decoder.evaluate(&current, &next, step, &mut evaluations);
        // console_log!("after decoder,evaluations is {:?}",evaluations);
        self.stack.evaluate(&current, &next, step, &mut evaluations[self.decoder.constraint_count()..]);


        console_log!("after stack,evaluations is {:?}",evaluations);
        console_log!("self.should_evaluate_to_zero_at(step) is {:?}",self.should_evaluate_to_zero_at(step));

        // when in debug mode, save transition evaluations before they are combined
        #[cfg(debug_assertions)]
        self.save_transition_evaluations(&evaluations, step);

        // if the constraints should evaluate to all zeros at this step,
        // make sure they do, and return
        // 所有的constraints 应该在这一 step 等于0
        if self.should_evaluate_to_zero_at(step) { // 8的倍数，应该是真实的数值
            let step = step / self.extension_factor;
            for i in 0..evaluations.len() {
                assert!(evaluations[i] == field::ZERO, "transition constraint at step {} were not satisfied", step);
            }
            return field::ZERO;
        }

        // compute a pseudo-random linear combination of all transition constraints
        return self.combine_transition_constraints(&evaluations, x);
    }

    /// Computes pseudo-random liner combination of transition constraints at point x. This function
    /// is similar to the one above but it can also be used to evaluate constraints at any point
    /// in the filed (not just in the evaluation domain). However, it is also much slower.
    pub fn evaluate_transition_at(&self, current: &TraceState, next: &TraceState, x: u128) -> u128 {
        // evaluate transition constraints
        let mut evaluations = vec![field::ZERO; self.t_constraint_num];
        self.decoder.evaluate_at(&current, &next, x, &mut evaluations);
        self.stack.evaluate_at(&current, &next, x, &mut evaluations[self.decoder.constraint_count()..]);

        // compute a pseudo-random linear combination of all transition constraints
        return self.combine_transition_constraints(&evaluations, x);
    }

    /// Computes pseudo-random linear combination of boundary constraints B_i at point x  separately
    /// for the first and for the last steps of the program; the constraints are computed as:
    /// cc_{i * 2} * B_i + cc_{i * 2 + 1} * B_i * x^p for all i, where cc_j are the coefficients
    /// used in the linear combination and x^p is a degree adjustment factor.
    pub fn evaluate_boundaries(&self, current: &TraceState, x: u128) -> (u128, u128) {
        
        // compute degree adjustment factor
        let xp = field::exp(x, self.b_degree_adj); // 约束 乘以 这个 度调整因子 ，会变成度都为|D_ev|- |D_trace|的多项式

        // 🤔 这里是3/10的公式 C(x) = Ek( cc_{i * 2} * B_i + cc_{i * 2 + 1} * B_i * x^p ) 这是对于所有的boundary constraint的公式

        // 1 ----- compute combination of boundary constraints for the first step ------------------
        let mut i_result = field::ZERO;
        let mut result_adj = field::ZERO;

        // 这里 cc 是随机产生的系数，是根据先前的随机数随机生成的系数的集合
        let cc = &self.coefficients.i_boundary;


        // 🈲️ 第一个 是对op_counter 的constraint
        // 怎么理解这个constraint呢？ 只有第一次op_counter 是 0 ，之后就是 6个中间值， 1， 6个中间值 ， 2 .....

        // make sure op_counter is set to 0
        let op_counter = current.op_counter();
        i_result = field::add(i_result, field::mul(op_counter, cc.op_counter[0]));
        result_adj = field::add(result_adj, field::mul(op_counter, cc.op_counter[1]));


        // make sure operation sponge registers are set to 0s
        let sponge = current.sponge();
        for i in 0..sponge.len() {
            i_result = field::add(i_result, field::mul(sponge[i], cc.sponge[i * 2]));
            result_adj = field::add(result_adj, field::mul(sponge[i], cc.sponge[i * 2 + 1]));
        }

        // make sure cf_bits are set to HACC (000)
        let mut cc_idx = 0;
        let op_bits = current.cf_op_bits();
        for i in 0..op_bits.len() {
            i_result = field::add(i_result, field::mul(op_bits[i], cc.op_bits[cc_idx]));
            result_adj = field::add(result_adj, field::mul(op_bits[i], cc.op_bits[cc_idx + 1]));
            cc_idx += 2;
        }

        // make sure low-degree op_bits are set to BEGIN (0000)
        let op_bits = current.ld_op_bits();
        for i in 0..op_bits.len() {
            i_result = field::add(i_result, field::mul(op_bits[i], cc.op_bits[cc_idx]));
            result_adj = field::add(result_adj, field::mul(op_bits[i], cc.op_bits[cc_idx + 1]));
            cc_idx += 2;
        }

        // make sure high-degree op_bits are set to BEGIN (00)
        let op_bits = current.hd_op_bits();
        for i in 0..op_bits.len() {
            i_result = field::add(i_result, field::mul(op_bits[i], cc.op_bits[cc_idx]));
            result_adj = field::add(result_adj, field::mul(op_bits[i], cc.op_bits[cc_idx + 1]));
            cc_idx += 2;
        }

        // make sure all context stack registers are 0s
        let ctx_stack = current.ctx_stack();
        console_log!("ctx_stack[0] is {:?}, ctx_stack.len is {:?}",ctx_stack[0],ctx_stack.len()); //0 ,len = 1 
        for i in 0..ctx_stack.len() {
            i_result = field::add(i_result, field::mul(ctx_stack[i], cc.ctx_stack[i * 2]));
            result_adj = field::add(result_adj, field::mul(ctx_stack[i], cc.ctx_stack[i * 2 + 1]));
        }
        // make sure all loop stack registers are 0s
        let loop_stack = current.loop_stack();
        console_log!("loop_stack[0] is {:?}, loop_stack.len is {:?}",loop_stack[0],loop_stack.len()); // 0 ,len = 1

        for i in 0..loop_stack.len() {
            i_result = field::add(i_result, field::mul(loop_stack[i], cc.loop_stack[i * 2]));
            result_adj = field::add(result_adj, field::mul(loop_stack[i], cc.loop_stack[i * 2 + 1]));
        }

        // make sure stack registers are set to inputs
        let user_stack = current.user_stack();
        for i in 0..self.inputs.len() {
            let val = field::sub(user_stack[i], self.inputs[i]);
            i_result = field::add(i_result, field::mul(val, cc.user_stack[i * 2]));
            result_adj = field::add(result_adj, field::mul(val, cc.user_stack[i * 2 + 1]));
        }

        // raise the degree of adjusted terms and sum all the terms together
        i_result = field::add(i_result, field::mul(result_adj, xp));

        // 2 ----- compute combination of boundary constraints for the last step -------------------
        let mut f_result = field::ZERO;
        let mut result_adj = field::ZERO;

        let cc = &self.coefficients.f_boundary;
        
        // make sure op_counter register is set to the claimed value of operations
        let val = field::sub(current.op_counter(), self.op_count); // 第一次是0 - 174， 然后是6个 很大的数-174， 然后是 1 - 174
        console_log!("val is {:?}, at current.op_counter = {:?}",val,current.op_counter());
        f_result = field::add(f_result, field::mul(val, cc.op_counter[0]));
        result_adj = field::add(result_adj, field::mul(val, cc.op_counter[1]));
        console_log!("i know f can be {:?}",f_result);
        // make sure operation sponge contains program hash
        let program_hash = current.program_hash();
        for i in 0..self.program_hash.len() {
            let val = field::sub(program_hash[i], self.program_hash[i]);
            f_result = field::add(f_result, field::mul(val, cc.sponge[i * 2]));
            result_adj = field::add(result_adj, field::mul(val, cc.sponge[i * 2 + 1]));
        }

        // make sure control flow op_bits are set VOID (111)
        let mut cc_idx = 0;
        let op_bits = current.cf_op_bits();
        for i in 0..op_bits.len() {
            let val = field::sub(op_bits[i], field::ONE);
            f_result = field::add(f_result, field::mul(val, cc.op_bits[cc_idx]));
            result_adj = field::add(result_adj, field::mul(val, cc.op_bits[cc_idx + 1]));
            cc_idx += 2;
        }
        
        // make sure low-degree op_bits are set to NOOP (11111)
        let op_bits = current.ld_op_bits();
        for i in 0..op_bits.len() {
            let val = field::sub(op_bits[i], field::ONE);
            f_result = field::add(f_result, field::mul(val, cc.op_bits[cc_idx]));
            result_adj = field::add(result_adj, field::mul(val, cc.op_bits[cc_idx + 1]));
            cc_idx += 2;
        }
        
        // make sure high-degree op_bits are set to NOOP (11)
        let op_bits = current.hd_op_bits();
        for i in 0..op_bits.len() {
            let val = field::sub(op_bits[i], field::ONE);
            f_result = field::add(f_result, field::mul(val, cc.op_bits[cc_idx]));
            result_adj = field::add(result_adj, field::mul(val, cc.op_bits[cc_idx + 1]));
            cc_idx += 2;
        }
        
        // make sure all context stack registers are 0s
        let ctx_stack = current.ctx_stack();
        for i in 0..ctx_stack.len() {
            f_result = field::add(f_result, field::mul(ctx_stack[i], cc.ctx_stack[i * 2]));
            result_adj = field::add(result_adj, field::mul(ctx_stack[i], cc.ctx_stack[i * 2 + 1]));
        }

        // make sure all loop stack registers are 0s
        let loop_stack = current.loop_stack();
        for i in 0..loop_stack.len() {
            f_result = field::add(f_result, field::mul(loop_stack[i], cc.loop_stack[i * 2]));
            result_adj = field::add(result_adj, field::mul(loop_stack[i], cc.loop_stack[i * 2 + 1]));
        }

        // make sure user stack registers are set to outputs
        for i in 0..self.outputs.len() {
            let val = field::sub(user_stack[i], self.outputs[i]);
            f_result = field::add(f_result, field::mul(val, cc.user_stack[i * 2]));
            result_adj = field::add(result_adj, field::mul(val, cc.user_stack[i * 2 + 1]));
        }        

        // raise the degree of adjusted terms and sum all the terms together
        f_result = field::add(f_result, field::mul(result_adj, xp));

        return (i_result, f_result);
    }

    // HELPER METHODS
    // -------------------------------------------------------------------------------------------
    fn should_evaluate_to_zero_at(&self, step: usize) -> bool {
        return (step & (self.extension_factor - 1) == 0) // same as: step % extension_factor == 0
            && (step != self.domain_size - self.extension_factor);
    }

    fn combine_transition_constraints(&self, evaluations: &Vec<u128>, x: u128) -> u128 {
        // 将所有的transition contraints 整合起来，并在 x 点求值
        let cc = &self.coefficients.transition;
        let mut result = field::ZERO;

        let mut i = 0;
        for (incremental_degree, constraints) in self.t_degree_groups.iter() {

            // for each group of constraints with the same degree, separately compute
            // combinations of D(x) and D(x) * x^p
            let mut result_adj = field::ZERO;
            for &constraint_idx in constraints.iter() {
                let evaluation = evaluations[constraint_idx];
                result = field::add(result, field::mul(evaluation, cc[i * 2]));
                result_adj = field::add(result_adj, field::mul(evaluation, cc[i * 2 + 1]));
                i += 1;
            }

            // increase the degree of D(x) * x^p
            let xp = field::exp(x, *incremental_degree);
            result = field::add(result, field::mul(result_adj, xp));
        }

        return result;
    }

    #[cfg(debug_assertions)]
    fn save_transition_evaluations(&self, evaluations: &[u128], step: usize) {
        unsafe {
            let mutable_self = &mut *(self as *const _ as *mut Evaluator);
            for i in 0..evaluations.len() {
                mutable_self.t_evaluations[i][step] = evaluations[i];
            }
        }
    }

    #[cfg(debug_assertions)]
    pub fn get_transition_evaluations(&self) -> &Vec<Vec<u128>> {
        return &self.t_evaluations;
    }

    #[cfg(debug_assertions)]
    pub fn get_transition_degrees(&self) -> Vec<usize> {
        return [
            self.decoder.constraint_degrees(), self.stack.constraint_degrees()
        ].concat();
    }
}

// HELPER FUNCTIONS
// ================================================================================================
fn group_transition_constraints(degrees: Vec<usize>, trace_length: usize) -> Vec<(u128, Vec<usize>)> {
    // 传入参数 34个元素的数组， 256
    // [2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 8, 8, 6, 4, 6, 7, 6, 6, 4, 4, 4 + 12个7] 
    let mut groups = [
        Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        Vec::new(), Vec::new(), Vec::new(), Vec::new(),
    ];

    // 这个for循环后，将度数分组, group[2]:[0,1,2,3,4,5...], group[3]:[10], 按照度数分类
    for (i, &degree) in degrees.iter().enumerate() {
        groups[degree].push(i);
    }
    console_log!("groups is {:?}",groups);
    
    let target_degree = get_transition_constraint_target_degree(trace_length); //计算结果target_degree = 2047

    // 这个result数组里面装的是元组！result[0] 是当degree为2的时候进去的， incremental_degree为 2047 - 255 * 【2】 =...
    let mut result = Vec::new();
    for (degree, constraints) in groups.iter().enumerate() {
        if constraints.len() == 0 { continue; }
        let constraint_degree = (trace_length - 1) * degree;    
        let incremental_degree = (target_degree - constraint_degree) as u128;
        result.push((incremental_degree, constraints.clone()));
    }
    return result;
}

fn get_boundary_constraint_adjustment_degree(trace_length: usize) -> u128 {
    let target_degree = get_boundary_constraint_target_degree(trace_length); // 7 * 256 +1   
    let boundary_constraint_degree = trace_length - 1; // 255 边界多项式的degree是 255
    return (target_degree - boundary_constraint_degree) as u128; //  调整因子就是 target degree - c_k(x) degree
}

/// target degree for boundary constraints is set so that when divided by boundary
/// constraint divisor (degree 1 polynomial), the degree will be equal to
/// deg(combination domain) - deg(trace)
/// 边界约束的目标度 ： 当被边界约束除数（度为1度多项式）除掉的时候，degree等于 deg（组合domian） - deg （trace） 
/// 
/// 边界的target degree 是  |D_ev| - |D_trace| +1 ,在这里就是 7 * 256 + 1
fn get_boundary_constraint_target_degree(trace_length: usize) -> usize {

    // 传入的值是 256
    let combination_degree = (MAX_CONSTRAINT_DEGREE - 1) * trace_length; // 7 * 256
    let divisor_degree = 1;
    return combination_degree + divisor_degree;// 7 * 256 + 1 
}

/// target degree for transition constraints is set so when divided transition 
/// constraint divisor (deg(trace) - 1 polynomial), the degree will be equal to
/// deg(combination domain) - deg(trace)
fn get_transition_constraint_target_degree(trace_length: usize) -> usize {
    // 传入的参数值为 256
    let combination_degree = (MAX_CONSTRAINT_DEGREE - 1) * trace_length; // （8-1）*256
    let divisor_degree = trace_length - 1; // 256 - 1 = 255
    return combination_degree + divisor_degree; // 两个相加 7 * 256 + 255 
}

fn parse_program_hash(program_hash: &[u8; 32]) -> Vec<u128> {
    return vec![
        field::from_bytes(&program_hash[..16]),
        field::from_bytes(&program_hash[16..]),
    ];
}

fn get_boundary_constraint_num(inputs: &[u128], outputs: &[u128]) -> usize {
    console_log!("input.len is {:?}, ouput.len is {:?}",inputs.len(),outputs.len());
    return
        PROGRAM_DIGEST_SIZE 
        + inputs.len() + outputs.len() // 1 , 8 
        + 1 /* for op_count */;
}