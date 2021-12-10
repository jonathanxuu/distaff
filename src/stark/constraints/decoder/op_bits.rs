use super::{
    field::{ self, mul, add },
    TraceState, FlowOps, UserOps, is_binary, binary_not, are_equal, EvaluationResult,
    CYCLE_MASK_IDX, PREFIX_MASK_IDX, PUSH_MASK_IDX,
};
use wasm_bindgen_test::console_log;

// CONSTRAINT EVALUATOR
// ================================================================================================

pub fn enforce_op_bits(result: &mut [u128], current: &TraceState, next: &TraceState, masks: &[u128; 3])
{
    // result 包括15个元素，current是当前步骤状态，next是下一步骤，masks 是 CYCLE MASKS （constriant/decoder里面的mask）
    let mut i = 0;

    // make sure all op bits are binary and compute their product/sum
    let mut cf_bit_sum = 0;
    for &op_bit in current.cf_op_bits() { //3个
        result[i] = is_binary(op_bit);  // 如果是二进制的，（即是原位的值），🌹 约束满足！那么result结果就是0
        cf_bit_sum = add(cf_bit_sum, op_bit); // cf_bit_sum 最终变为3 位cf位的数字和
        i += 1;
    }

    let mut ld_bit_prod = 1;
    for &op_bit in current.ld_op_bits() {//5 个
        result[i] = is_binary(op_bit); // 如果是二进制的，（即是原位的值），🌹 约束满足！那么result结果就是0
        ld_bit_prod = mul(ld_bit_prod, op_bit); // ld_bit_prod 是5位ld_bit位的乘积，一旦有一个是0，那么这个乘积就是0
        i += 1;
    }

    let mut hd_bit_prod = 1;
    for &op_bit in current.hd_op_bits() {//2 个
        result[i] = is_binary(op_bit);// 如果是二进制的，（即是原位的值），🌹 约束满足！那么result结果就是0
        hd_bit_prod = mul(hd_bit_prod, op_bit);  //hd_bit_prod 是hd_bit位的乘积，一旦有一个是0，那么这个乘积就是0
        i += 1;
    }

    // when cf_ops = hacc, operation counter should be incremented by 1;
    // otherwise, operation counter should remain the same
    // 当 cf_ops 是hacc， ————换句话说，也就是 cf_bit_sum = 0
    // 那么operation counter 应当加1，否则operation counter不变 
    let op_counter = current.op_counter();
    console_log!("im in enforce, current.ld_op_flags is {:?}",current.ld_op_flags);
    let is_hacc = current.cf_op_flags()[FlowOps::Hacc.op_index()];  // 后面的index始终是 0 —— 因为hacc的index是 0
    // 只要是本案例中的 恰时步骤，就应当是 HACC，所以is hacc = 1

    let hacc_transition = mul(add(op_counter, field::ONE), is_hacc); //如果是hacc， (op_counter + 1) * is_hacc   一开始 是 1，..., 2
    let rest_transition = mul(op_counter, binary_not(is_hacc)); // 如果是hacc 则rest_transition 就是 0；如果非hacc，那么rest_transition 非 0
    result[i] = are_equal(add(hacc_transition, rest_transition), next.op_counter()); // 如果是一个 🌹约束满足！恰时操作，则 (a+b) = v2 ，即result [i] = 0  (第十一个）
    i += 1;

    // ld_ops and hd_ops can be all 0s at the first step, but cannot be all 0s
    // at any other step
    result[i] = mul(op_counter, mul(binary_not(ld_bit_prod), binary_not(hd_bit_prod)));// 若ld中有一个是0，则prod为0，则binary_not为1， 若hd中有一个是0，则prod为0，则binary_not为1
    i += 1;                                                                                   // 第十二个
 
    // when cf_ops are not all 0s, ld_ops and hd_ops must be all 1s
    result[i] = mul(cf_bit_sum, binary_not(mul(ld_bit_prod, hd_bit_prod)));  // 第十三个
    i += 1;
    console_log!("im in enforce2, current.ld_op_flags is {:?}",current.ld_op_flags);
    let cf_op_flags = current.cf_op_flags();//💗 1000 0000 是HACC对应得到的cf_op_flags

    // VOID can be followed only by VOID
    let current_void_flag = cf_op_flags[FlowOps::Void.op_index()]; // 判断是不是void？ 若是则为1
    console_log!("im in enforce, next.ld_op_flags is {:?}",next.ld_op_flags);
    let next_void_flag = next.cf_op_flags()[FlowOps::Void.op_index()];// 判断next 是不是void 若是则为1
    result[i] = mul(current_void_flag, binary_not(next_void_flag)); // 若满足 相同，则🌹满足约束～ 所以result = 0 // 第十四个
    i += 1;

    let hd_op_flags = current.hd_op_flags();
    // 0,0 对应的是 【1，0，0，0】 （push和begin）  🤔️ 猜测 表示9？
    // 1，1 对应的是【0 0 0 1】 （low degree的） 猜测 表示2？
    // 1 0 对应的是 【0 1 0 0】 （实际上是0 1—— cmp） 猜测 表示 5？
                //💗 1000 0000 是HACC对应得到的cf_op_flags
    // BEGIN, LOOP, BREAK, and WRAP are allowed only on one less than multiple of 16
    console_log!("masks is {:?}",masks);
    let prefix_mask = masks[PREFIX_MASK_IDX];
    result.agg_constraint(i, cf_op_flags[FlowOps::Begin.op_index()], prefix_mask); // index为[1]
    result.agg_constraint(i, cf_op_flags[FlowOps::Loop.op_index()],  prefix_mask); // index为[4]
    result.agg_constraint(i, cf_op_flags[FlowOps::Wrap.op_index()],  prefix_mask); // index为[5]
    result.agg_constraint(i, cf_op_flags[FlowOps::Break.op_index()], prefix_mask); // index为[6]

    // 在16的整数倍 - 1，应当放这四种
     // 在16的倍数 -1 ，应当放这四种，如果是16的倍数-1，那么后面value的值应当为0

    // fn agg_constraint(&mut self, index: usize, flag: u128, value: u128) {
    //     self[index] = field::add(self[index], field::mul(flag, value));
    // }

    // TEND and FEND is allowed only on multiples of 16
    // 在16的倍数，应当放这两种，如果是16的倍数，那么后面两个value的值应当为0

    let base_cycle_mask = masks[CYCLE_MASK_IDX];
    result.agg_constraint(i, cf_op_flags[FlowOps::Tend.op_index()], base_cycle_mask);
    result.agg_constraint(i, cf_op_flags[FlowOps::Fend.op_index()], base_cycle_mask);

    // PUSH is allowed only on multiples of 8
    let push_cycle_mask = masks[PUSH_MASK_IDX];
    result.agg_constraint(i, hd_op_flags[UserOps::Push.hd_index()], push_cycle_mask);
}

// TESTS
// ================================================================================================
#[cfg(test)]
mod tests {

    use super::{ TraceState, FlowOps, UserOps, super::NUM_OP_CONSTRAINTS };

    #[test]
    fn op_bits_are_binary() {

        let success_result = vec![0; NUM_OP_CONSTRAINTS];

        // all bits are 1s: success
        let state = new_state(FlowOps::Void as u8, UserOps::Noop as u8, 1);
        assert_eq!(success_result, evaluate_state(&state, [0, 0, 0], false));

        // control flow bits are not binary
        for i in 0..3 {
            let mut op_bits = [1; 3];
            op_bits[i] = 3;
            let mut expected_evaluations = vec![0; 10];
            expected_evaluations[i] = 3 * 3 - 3;

            let state = new_state_from_bits(op_bits, [1, 1, 1, 1, 1, 1, 1]);
            assert_eq!(expected_evaluations, &evaluate_state(&state, [0, 0, 0], false)[..10]);
        }

        // user bits are not binary
        for i in 0..7 {
            let mut op_bits = [1, 1, 1, 1, 1, 1, 1];
            op_bits[i] = 3;
            let mut expected_evaluations = vec![0; 10];
            expected_evaluations[i + 3] = 3 * 3 - 3;

            let state = new_state_from_bits([0, 0, 0], op_bits);
            assert_eq!(expected_evaluations, &evaluate_state(&state, [0, 0, 0], false)[..10]);
        }
    }

    #[test]
    fn invalid_op_combinations() {

        let success_result = vec![0; NUM_OP_CONSTRAINTS];

        // user op bits cannot be all 0s
        for cf_op in 0..8 {
            let state = new_state(cf_op, 0, 1);
            assert_ne!(success_result, evaluate_state(&state, [0, 0, 0], false));
        }

        // when cf_ops are not all 0s, user_ops must be all 1s
        for cf_op in 1..8 {
            for user_op in 0..127 {
                let state = new_state(cf_op as u8, user_op as u8, 1);
                assert_ne!(success_result, evaluate_state(&state, [0, 0, 0], false));
            }

            let state = new_state(cf_op as u8, UserOps::Noop as u8, 1);
            assert_eq!(success_result, evaluate_state(&state, [0, 0, 0], false));
        }
    }

    #[test]
    fn invalid_op_alignment() {

        let success_result = vec![0; NUM_OP_CONSTRAINTS];
        
        // TEND and FEND are allowed only on multiples of 16
        let state = new_state(FlowOps::Tend as u8, UserOps::Noop as u8, 1);
        assert_eq!(success_result, evaluate_state(&state, [0, 0, 0], false));
        assert_ne!(success_result, evaluate_state(&state, [1, 0, 0], false));

        let state = new_state(FlowOps::Fend as u8, UserOps::Noop as u8, 1);
        assert_eq!(success_result, evaluate_state(&state, [0, 0, 0], false));
        assert_ne!(success_result, evaluate_state(&state, [1, 0, 0], false));

        // BEGIN, LOOP, WRAP, and BREAK are allowed only on one less than multiples of 16
        let state = new_state(FlowOps::Begin as u8, UserOps::Noop as u8, 1);
        assert_eq!(success_result, evaluate_state(&state, [0, 0, 0], false));
        assert_ne!(success_result, evaluate_state(&state, [0, 1, 0], false));

        let state = new_state(FlowOps::Loop as u8, UserOps::Noop as u8, 1);
        assert_eq!(success_result, evaluate_state(&state, [0, 0, 0], false));
        assert_ne!(success_result, evaluate_state(&state, [0, 1, 0], false));

        let state = new_state(FlowOps::Wrap as u8, UserOps::Noop as u8, 1);
        assert_eq!(success_result, evaluate_state(&state, [0, 0, 0], false));
        assert_ne!(success_result, evaluate_state(&state, [0, 1, 0], false));

        let state = new_state(FlowOps::Break as u8, UserOps::Noop as u8, 1);
        assert_eq!(success_result, evaluate_state(&state, [0, 0, 0], false));
        assert_ne!(success_result, evaluate_state(&state, [0, 1, 0], false));

        // PUSH is allowed only on multiples of 8
        let state = new_state(FlowOps::Hacc as u8, UserOps::Push as u8, 1);
        assert_eq!(success_result, evaluate_state(&state, [0, 0, 0], true));
        assert_ne!(success_result, evaluate_state(&state, [0, 0, 1], true));
    }

    #[test]
    fn invalid_op_sequence() {
        let success_result = vec![0; NUM_OP_CONSTRAINTS];

        // void can follow non-void
        let state1 = new_state(FlowOps::Hacc as u8, UserOps::Add as u8, 1);
        let state2 = new_state(FlowOps::Void as u8, UserOps::Noop as u8, 2);
        let mut evaluations = vec![0; NUM_OP_CONSTRAINTS];
        super::enforce_op_bits(&mut evaluations, &state1, &state2, &[0, 0, 0]);
        assert_eq!(success_result, evaluations);

        // void can follow void
        let state1 = new_state(FlowOps::Void as u8, UserOps::Noop as u8, 1);
        let state2 = new_state(FlowOps::Void as u8, UserOps::Noop as u8, 1);
        let mut evaluations = vec![0; NUM_OP_CONSTRAINTS];
        super::enforce_op_bits(&mut evaluations, &state1, &state2, &[0, 0, 0]);
        assert_eq!(success_result, evaluations);

        // non-void cannot follow void
        let state1 = new_state(FlowOps::Void as u8, UserOps::Noop as u8, 1);
        let state2 = new_state(FlowOps::Hacc as u8, UserOps::Add as u8, 1);
        let mut evaluations = vec![0; NUM_OP_CONSTRAINTS];
        super::enforce_op_bits(&mut evaluations, &state1, &state2, &[0, 0, 0]);
        assert_ne!(success_result, evaluations);
    }

    // HELPER FUNCTIONS
    // --------------------------------------------------------------------------------------------
    fn new_state(flow_op: u8, user_op: u8, op_counter: u128) -> TraceState {
        let mut state = TraceState::new(1, 0, 1);
    
        let mut op_bits = [0; 10];
        for i in 0..3 {
            op_bits[i] = ((flow_op as u128) >> i) & 1;
        }
    
        for i in 0..7 {
            op_bits[i + 3] = ((user_op as u128) >> i) & 1;
        }

        state.set_op_bits(op_bits);
        state.set_op_counter(op_counter);
        return state;
    }

    fn new_state_from_bits(cf_bits: [u128; 3], u_bits: [u128; 7]) -> TraceState {
        let mut state = TraceState::new(1, 0, 1);
        state.set_op_bits([
            cf_bits[0], cf_bits[1], cf_bits[2],
            u_bits[0], u_bits[1], u_bits[2], u_bits[3], u_bits[4], u_bits[5], u_bits[6]
        ]);
        return state;
    }

    fn evaluate_state(state: &TraceState, masks: [u128; 3], inc_counter: bool) -> Vec<u128> {
        let op_counter = if inc_counter { state.op_counter() + 1 } else { state.op_counter() };
        let next_state = new_state(FlowOps::Void as u8, UserOps::Noop as u8, op_counter);
        let mut evaluations = vec![0; NUM_OP_CONSTRAINTS];
        super::enforce_op_bits(&mut evaluations, &state, &next_state, &masks);
        return evaluations;
    }
}