#![cfg(test)]

use super::*;
use soroban_sdk::{vec, Env, String, testutils::{Address as _, Ledger as _}};


// Setup function to initialise the DAO
fn setup() -> (DAOContractClient<'static>, Address, Env, Address, Vec<Address>) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let members = vec![
        &env,
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    let token_id = env.register_stellar_asset_contract(admin.clone());
    let contract_id = env.register_contract(None, DAOContract);
    let client = DAOContractClient::new(&env, &contract_id);

    env.mock_all_auths();
    client.initialise(&admin, &members, &token_id);
    (client, admin, env, token_id, members)
}

// Test proposal creation
#[test]
fn test_proposal_creation() {
    let (client, _, env, _, members) = setup();

    env.mock_all_auths();

    // Create a proposal
    let creator = members.get(0).unwrap();
    let deadline = env.ledger().timestamp() + 86400; // 1 day in future
    let proposal_id = client.create_proposal(
        &creator,
        &String::from_str(&env, "Funding Initiative"),
        &String::from_str(&env, "Fund new project"),
        &deadline,
    );

    // Verify proposal details
    let proposal = client.get_proposal(&proposal_id);
    assert_eq!(proposal.id, 1);
    assert_eq!(proposal.title, String::from_str(&env, "Funding Initiative"));
    assert_eq!(proposal.description, String::from_str(&env, "Fund new project"));
    assert_eq!(proposal.deadline, deadline);
    assert_eq!(proposal.yes_votes, 0);
    assert_eq!(proposal.no_votes, 0);
    assert_eq!(proposal.creator, creator);
    assert_eq!(proposal.active, true);

    // Verify proposal count
    let proposals = client.list_proposals();
    assert_eq!(proposals.len(), 1);
}

// Test voting
#[test]
fn test_voting() {
    let (client, _, env, token_id, members) = setup();

    env.mock_all_auths();

    // Create a proposal
    let creator = members.get(0).unwrap();
    let deadline = env.ledger().timestamp() + 86400;
    let proposal_id = client.create_proposal(
        &creator,
        &String::from_str(&env, "Funding Initiative"),
        &String::from_str(&env, "Fund new project"),
        &deadline,
    );

    // Member 1 votes yes with 50 tokens
    let voter1 = members.get(1).unwrap();
    client.vote(&voter1, &proposal_id, &true, &50_0000000);

    // Member 2 votes no with 30 tokens
    let voter2 = members.get(2).unwrap();
    client.vote(&voter2, &proposal_id, &false, &30_0000000);

    // Verify proposal vote counts
    let proposal = client.get_proposal(&proposal_id);
    assert_eq!(proposal.yes_votes, 50_0000000);
    assert_eq!(proposal.no_votes, 30_0000000);

    // Verify voter balances (tokens locked)
    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&voter1), 50_0000000); // 100 - 50
    assert_eq!(token_client.balance(&voter2), 70_0000000); // 100 - 30

    // Test duplicate voting
    let result = client.vote(&voter1, &proposal_id, &true, &10_0000000);
    assert_eq!(result, Err(DAOError::AlreadyVoted));

    // Test voting with insufficient tokens
    let result = client.vote(&voter1, &proposal_id, &true, &100_0000000);
    assert_eq!(result, Err(DAOError::InsufficientTokens));
}

// Test tallying and unlocking
#[test]
fn test_tallying_and_unlocking() {
    let (client, _, env, token_id, members) = setup();

    env.mock_all_auths();

    // Create a proposal
    let creator = members.get(0).unwrap();
    let deadline = env.ledger().timestamp() + 86400;
    let proposal_id = client.create_proposal(
        &creator,
        &String::from_str(&env, "Funding Initiative"),
        &String::from_str(&env, "Fund new project"),
        &deadline,
    );

    // Members vote
    let voter1 = members.get(1).unwrap();
    let voter2 = members.get(2).unwrap();
    client.vote(&voter1, &proposal_id, &true, &60_0000000);
    client.vote(&voter2, &proposal_id, &false, &40_0000000);

    // Try to tally before deadline
    let result = client.tally_proposal(&proposal_id);
    assert_eq!(result, Err(DAOError::VotingNotClosed));

    // Advance ledger time past deadline
    env.ledger().with_mut(|l| l.timestamp += 86400 + 1);

    // Tally proposal
    let passed = client.tally_proposal(&proposal_id);
    assert_eq!(passed, true); // 60 yes > 40 no, quorum met (100 > 51% of 300)

    // Verify proposal is closed
    let proposal = client.get_proposal(&proposal_id);
    assert_eq!(proposal.active, false);

    // Unlock tokens for voter1
    client.unlock_tokens(&voter1);

    // Verify voter1 balance restored
    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&voter1), 100_0000000); // Full balance restored

    // Verify locked tokens cleared
    let locked: Option<i128> = env.storage().instance().get(&DataKey::LockedTokens(voter1.clone()));
    assert_eq!(locked, None);

    // Test unlocking with no locked tokens
    client.unlock_tokens(&voter1); // Should not panic
}