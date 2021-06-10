use chrono::DateTime;
use nimiq_account::{Account, BasicAccount, StakingContract};
use nimiq_accounts::Accounts;

use std::env;
use std::process::exit;
use std::usize;
pub use lmdb_zero::open;

use std::convert::TryFrom;

use std::sync::Arc;
use chrono::{Utc};
use nimiq_vrf::VrfSeed;

use std::fs::File;
use std::io::{prelude::*, BufReader};


use nimiq_lib::config::config::DatabaseConfig;

use nimiq_hash::{Blake2bHash, Blake2sHasher, Hash, Hasher};
use nimiq_block::{Block, MacroBlock, MacroBody, MacroHeader};
use nimiq_blockchain::{ChainInfo,ChainStore};


use nimiq_database::{
    lmdb::{LmdbEnvironment},     
    Environment, 
    WriteTransaction,      
};

use nimiq_keys::Address;

/* Should at least be able to contain a genesis block and validators */
const BATCH_SIZE: i32 = 1000;

mod config;

/* Stores the state while we iterate trough the toml file */
pub struct StateData{
    environment : Environment,
    accounts_tree: Accounts,    
    genesis_accounts: Vec<(Address, Account)>,
    stacking_contract: StakingContract,    
    seed: VrfSeed,
    timestamp: DateTime<Utc>,    
}

 
fn usage(args: Vec<String>) -> ! {
    eprintln!(
        "Usage: {} GENESIS_FILE",
        args.get(0).unwrap_or(&String::from("nimiq-genesis"))
    );
    exit(1);
}

/* This function processes a batch of toml data*/
fn process_toml(buffer: &str,state: & mut StateData){

    /* De construct the toml content */
    let config::GenesisConfig {
        signing_key,
        seed_message,
        timestamp,
        validators,
        stakes,
        accounts,
        staking_contract,        
    } = toml::from_str(buffer).unwrap();

    /* If there is a seed message, we use it to construct the seed */
    if let  Some(s) = seed_message {

        // We assume that if we have a seed message, we also have the signing key        
        let pre_genesis_seed: VrfSeed = signing_key.unwrap().sign_hash(Blake2sHasher::new().digest(s.as_bytes())).compress().into();
        println!("Pre genesis seed: {}", pre_genesis_seed);

        /* Store the seed as part of our data */
        state.seed = pre_genesis_seed.sign_next(&signing_key.unwrap());
        println!("Genesis seed: {}", state.seed);
    }

    if let Some(t) = timestamp{
        state.timestamp = t.clone();
    }
    
    /* TODO:  It would be good to check that the amount of validators and stakes matches */

    /* Process validators (if any) */
    for validator in validators.iter().flatten() {
        state.stacking_contract.create_validator(
            validator.validator_id.clone(),
            validator.validator_key.compress(),
            validator.reward_address.clone(),
            validator.balance,
        );
    }

    /* Process the stake */
    for stake in stakes.iter().flatten() {
        state.stacking_contract.stake(
            stake.staker_address.clone(),
            stake.balance,
            &stake.validator_id,
        );
    }

    /* Obtain the <address,account> associated to the stacking contrat and push it to accounts*/
    if let Some(a) = staking_contract{
        state.genesis_accounts.push((
            Address::clone(&a),        
            Account::Staking(state.stacking_contract.clone()),
        ));
    }

    /* Process accounts (if any) */
    let mut accounts_counter: u32 = 0;

    /* This is where the majority of the work is done */
    for genesis_account in accounts.iter().flatten() {
        let address = genesis_account.address.clone();
        let account = Account::Basic(BasicAccount {
            balance: genesis_account.balance,
        });
        println!("  Processing account: {}: {:?}", address, account);        
        state.genesis_accounts.push((address, account));
        accounts_counter += 1;        
    }
    
    /* We create a new transaction with all the accounts data(if any) and 
       commit it to the database */
    let mut txn = WriteTransaction::new(&state.environment);
    /* This will append the accounts to the Accounts tree */
    state.accounts_tree.init(&mut txn, state.genesis_accounts.clone());
    txn.commit();

    println!(" --------- Done processing {} accounts ------------", accounts_counter);
}


// Process the toml file in batches of BATCH_SIZE size
fn process_file(file_name: &str, state: &mut StateData) {
    let file = File::open(file_name).expect("file not found!");
    let mut reader = BufReader::new(file); 

    let mut buffer_size = 0;
    let mut buffer = String::with_capacity((4*56*BATCH_SIZE) as usize);    
    //let mut buffer = String::new();
    let mut success = false;

    while let Ok(read_bytes) = reader.read_line(&mut buffer) {
        if read_bytes != 0 {     
            
            // This counts new lines (a new line contains a single char)
            if read_bytes == 1 {
                buffer_size += 1;                            
            }

            //We have a batch ready, so we pass it to the process_toml function
            if buffer_size == BATCH_SIZE {
                process_toml(&buffer,state);
                buffer.clear();     
                buffer_size = 0;           
            }
            
        } else {
            /* This is the case where we reached EOF, so we need to process the last data we obtained */
            success = true;
            process_toml(&buffer, state);
            break;
        }        
    }
    
    if !success {
        println!(" Something failed ")
    }

}


fn main() {
    let args = env::args().collect::<Vec<String>>();

    let file = match args.get(1) {
        None => {
            usage(args);            
        },
        Some(file) => file,
    };

    /* Database configuration */
    let DatabaseConfig{size, max_dbs, .. } = DatabaseConfig::default();

    let env = LmdbEnvironment::new("./test", size, max_dbs, open::NOTLS).unwrap();  

    /* Creates a new chain store to be used later */
    let chain_store = Arc::new(ChainStore::new(env.clone()));    

    /* Create a new accounts tree*/
    let accounts = Accounts::new(env.clone());

    /* Instiantiate our state structure with default values, this will be filled by process_file() */
    let mut state = StateData{     
        environment       : env.clone(),   
        accounts_tree     : accounts,        
        genesis_accounts  : Vec::new(),
        stacking_contract : StakingContract::default(),        
        timestamp         : Utc::now(),
        seed              : VrfSeed::default(),              
    };

    /* Process the toml file in chunks */
    process_file(file, &mut state);

    /* Create a transaction that will be used to commit the Genesis Block */
    let mut txn = WriteTransaction::new(&env);

    // Create slots allocation from stacking contract
    let slots = state.stacking_contract.select_validators(&state.seed);
    println!("Slots: {:#?}", slots);

    // Body for the Genesis Block
    let mut body = MacroBody::new();
    body.validators = Some(slots);
    let body_root = body.hash::<Blake2bHash>();
    println!("Body root: {}", &body_root);

    /* Accounts tree root hash */
    let state_root = state.accounts_tree.hash(Some(&txn));
    println!("State root: {}", &state_root);

    // MacroHeader for the Genesis Block
    let header = MacroHeader {
        version: 1,
        block_number: 0,
        view_number: 0,
        timestamp: u64::try_from(state.timestamp.timestamp_millis()).unwrap(),
        parent_hash: [0u8; 32].into(),
        parent_election_hash: [0u8; 32].into(),
        seed: state.seed,
        extra_data: vec![],
        state_root,
        body_root,
        history_root: Blake2bHash::default(),
    };

    /* Construct the Genesis Block */
    let genesis_block =  Block::Macro(MacroBlock {
        header,
        justification: None,
        body: Some(body)
    });

    let head_hash = genesis_block.hash();

    println!(" Head Hash: {}", &head_hash);

    let main_chain = ChainInfo::new(genesis_block, true);

    println!(" Storing data into Chain Store ");
    
    chain_store.put_chain_info(&mut txn, &head_hash, &main_chain, true);

    chain_store.set_head(&mut txn, &head_hash);

    txn.commit();

}
