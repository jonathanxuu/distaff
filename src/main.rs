use std::{ env, io::Write, time::Instant };
use distaff::{ self, StarkProof,GenOutput };
mod examples;
use examples::{ Example };
use distaff::{ ProgramInputs, assembly,ProofOptions };


fn main() {
    let a = assembly::compile("
    begin 
    push.0 push.1 push.2 push.4 
    push.1 read dup push.1 
    while.true
        roll.4 ne 
        if.true 
            swap push.1 add dup push.5 ne 
            if.true 
                swap dup push.1
            else
                pad.3 
            end
        else 
            push.1 pad
        end
    end
end").unwrap();    
    let b = ProgramInputs::new(&[], &[1], &[]);
    let c = 1;
    let d = ProofOptions::default();

    //         使用execute             //inputs 1,0   , 1     
    let res = distaff::execute(&a, &b, c, &d);
    let res_slice: &str = &res[..];
    let res_struct: GenOutput = serde_json::from_str(res_slice).unwrap();
    let outputs = res_struct.stark_output;
    let proof = res_struct.stark_proof;

    println!("--------------------------------");
    println!("Executed program with hash {}", 
        hex::encode(a.hash()));
    println!("Program output: {:?}", outputs);

    // serialize the proof to see how big it is
    println!("proof_bytes is: {:?}", proof.clone());
    // let proof_bytes = bincode::serialize(&proof).unwrap();
    let proof_slice: &str = &proof[..];
    
    let proof_b:Vec<u8> = serde_json::from_str(proof_slice).unwrap();

    println!("Execution proof size: {} KB", proof_b.len() / 1024);
    println!("Execution proof security: {} bits", d.security_level(true));
    println!("--------------------------------");
    

    // println!("Execution proof1 is:{:?}",proof_bytes);

    
    // verify that executing a program with a given hash and given inputs
    // results in the expected output
    let proof = bincode::deserialize::<StarkProof>(&proof_b).unwrap();

    // log::debug!("proof is {:?}",hex::encode(&proof_bytes));
    // log::debug!("public is {:?}",inputs.get_public_inputs());
    let now = Instant::now();
    println!("program.hash is {:?}, inputs pub is {:?},outputs is {:?}",a.hash(), b.get_public_inputs(), &outputs);

        match distaff::verify(a.hash(), b.get_public_inputs(), &outputs, &proof) {
        Ok(_) => println!("Execution verified in {} ms", now.elapsed().as_millis()),
        Err(msg) => println!("Failed to verify execution: {}", msg)
    }
}