use std::{ env, io::Write, time::Instant };
use distaff::{ self, StarkProof };
mod examples;
use examples::{ Example };


fn main() {

    // configure logging
    env_logger::Builder::new()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter_level(log::LevelFilter::Debug).init();

    // determine the example to run based on command-line inputs
    let ex: Example;
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        ex = examples::testrange::get_example(&args);
    }
    else {
        ex = match args[1].as_str() {
            "stacktest"      => examples::stacktest::get_example(&args[1..]),
            "testrange"      => examples::testrange::get_example(&args[1..]),
            "collatz"       => examples::collatz::get_example(&args[1..]),
            "comparison"    => examples::comparison::get_example(&args[1..]),
            "conditional"   => examples::conditional::get_example(&args[1..]),
            "fibonacci"     => examples::fibonacci::get_example(&args[1..]),
            "merkle"        => examples::merkle::get_example(&args[1..]),
            "rangecheck"    => examples::range::get_example(&args[1..]),
            _ => panic!("Could not find example program for '{}'", args[1])
        }
    }
                //这里    inputs是1，0    , 1   ,           因为斐波那契数列 arg=1，第6个结果，所以expected_result是8
    let Example { program, inputs, num_outputs, options, expected_result } = ex;
    println!("--------------------------------");

    // execute the program and generate the proof of execution
    let now = Instant::now();
                //         使用execute             //inputs 1,0   , 1     
    let (outputs, proof) = distaff::execute(&program, &inputs, num_outputs, &options);
    println!("--------------------------------");
    println!("Executed program with hash {} in {} ms", 
        hex::encode(program.hash()),
        now.elapsed().as_millis());
    println!("Program output: {:?}", outputs);

    assert_eq!(expected_result, outputs, "Program result was computed incorrectly");
    // serialize the proof to see how big it is
    let proof_bytes = bincode::serialize(&proof).unwrap();
    println!("proof_bytes is: {:?}", proof_bytes.clone());

    println!("Execution proof size: {} KB", proof_bytes.len() / 1024);
    println!("Execution proof security: {} bits", options.security_level(true));
    println!("--------------------------------");
    

    // println!("Execution proof1 is:{:?}",proof_bytes);

    
    // verify that executing a program with a given hash and given inputs
    // results in the expected output
    let proof = bincode::deserialize::<StarkProof>(&proof_bytes).unwrap();

    // log::debug!("proof is {:?}",hex::encode(&proof_bytes));
    // log::debug!("public is {:?}",inputs.get_public_inputs());
    let now = Instant::now();
    println!("program.hash is {:?}, inputs pub is {:?},outputs is {:?}",program.hash(), inputs.get_public_inputs(), &outputs);

        match distaff::verify(program.hash(), inputs.get_public_inputs(), &outputs, &proof) {
        Ok(_) => println!("Execution verified in {} ms", now.elapsed().as_millis()),
        Err(msg) => println!("Failed to verify execution: {}", msg)
    }
}