#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, String, Vec, token};

// DAO contract Outline

// Admin
// -> Initialize DAO with token minting and distribution

// Token Managment Functions
// -> Get token balance
// -> Get total supply

// Proposal Managment Functions
// -> Create a proposal
// -> Get a proposal
// -> List all proposals

// Voting Managment Functions
// -> Vote on a proposal
// -> Tally proposal outcome
// -> Unlock tokens after deadline

// Things to take note
// Structure of data
// How to retreive data and store data
// How to get environment variables
// Ensure token conforms to Soroban token interface
// Handle voting deadlines and vote locking

// Structure for token configuration
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenConfig {
    total_supply: u64,
    admin: Address,
}

// Structure for a proposal
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    id: u64,
    title: String,
    description: String,
    deadline: u64,
    yes_votes: u64,
    no_votes: u64,
    creator: Address,
    active: bool,
}

// strucutre for a vote
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Vote {
    voter: Address,
    amount: u64,
    is_yes: bool, // True yes/ False no....
}

//Data key for Storage
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    TokenConfig,
    Proposal(u64),
    ProposalCount,
    Vote(u64, Address),
    LockedTokens(Address),
}

//Error codes
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DAOError {
    AlreadyInitialised = 1,
    NotInitialised = 2,
    InvalidProposal = 3,
    ProposalExpired = 4,
    AlreadyVoted = 5,
    InsufficientTokens = 6,
    VotingNotClosed = 7,
    InvalidVote = 8,
    
}

// Implement conversion from DAOError to soroban_sdk::Error
impl From<DAOError> for soroban_sdk::Error {
    fn from(error: DAOError) -> Self {
        soroban_sdk::Error::from_type_and_code(
            soroban_sdk::xdr::ScErrorType::Contract,
            error,
        )
    }
}

// Implement conversion for references
impl<'a> From<&'a DAOError> for soroban_sdk::Error {
    fn from(error: &'a DAOError) -> Self {
        (*error).into()
    }
}

#[contract]
pub struct DAOContract;

#[contractimpl]
impl DAOContract {
    //Initialiser for minitng and distribution of tokens
    pub fn initialise(
        env: Env,
        admin: Address,
        members: Vec<Address>,
        token_id: Address,
    ) -> Result<(), DAOError> {
        if env.storage().instance().has(&DataKey::TokenConfig) {
            return Err(DAOError::AlreadyInitialised);
        }

        // set Admin and authorisation
        admin.require_auth();

        //calculate total supply of tokens per member
        let total_supply = members.len() as u64 * 100 * 100000;

        //Store token configuration
        let token_config = TokenConfig {
            total_supply,
            admin: admin.clone(),
        };

        env.storage()
            .instance()
            .set(&DataKey::TokenConfig, &token_config);

        //initialse proposal count
        env.storage().instance().set(&DataKey::ProposalCount, &0u64);

        //Mint and Distribute tokens to members
        let token_client = token::Client::new(&env, &token_id);
        for member in members.iter() {
            token_client.transfer(&admin, &member, &(100 * 100000));
        }

        Ok(())
    }

    //Get token balance for an address
    pub fn balance(env: Env, account: Address) -> Result<u64, DAOError> {
        //Get token configuration
        let token_config: TokenConfig = env
            .storage()
            .instance()
            .get(&DataKey::TokenConfig)
            .ok_or(DAOError::NotInitialised)?;

        //Query token balance for an address
        let token_client = token::Client::new(&env, &token_config.admin);
        Ok(token_client.balance(&account).try_into().map_err(|_| DAOError::InvalidVote)?)
    }

    //Get total token supply
    pub fn total_supply(env: Env) -> Result<u64, DAOError> {
        //Get token configuration
        let token_config: TokenConfig = env
            .storage()
            .instance()
            .get(&DataKey::TokenConfig)
            .ok_or(DAOError::NotInitialised)?;

        Ok(token_config.total_supply)
    }

    //create a proposal
    pub fn create_proposal(
        env: Env,
        creator: Address,
        title: String,
        description: String,
        deadline: u64,
    ) -> Result<u64, DAOError> {
        //Require creator authorisation
        creator.require_auth();

        //Validate deadline (must be in the future)
        let current_time = env.ledger().timestamp();
        if deadline <= current_time {
            return Err(DAOError::ProposalExpired);
        }

        //Get and increment proposal count
        let proposal_count: u64 = env.storage().instance().get(&DataKey::ProposalCount).unwrap_or(0);
        let new_id = proposal_count + 1;

        //create new proposal
        let proposal = Proposal {
            id: new_id,
            title,
            description,
            deadline,
            yes_votes: 0,
            no_votes: 0,
            creator,
            active: true,
        };

        //Store proposal and update count
        env.storage()
            .instance()
            .set(&DataKey::Proposal(new_id), &proposal);
        env.storage()
            .instance()
            .set(&DataKey::ProposalCount, &new_id);

        Ok(new_id)
    }

    //Get a proposal by ID
    pub fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, DAOError> {
        env.storage()
            .instance()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(DAOError::InvalidProposal)
    }

    // List all proposals
    pub fn list_proposals(env: Env) -> Result<Vec<Proposal>, DAOError> {
        let proposal_count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ProposalCount)
            .unwrap_or(0);
        let mut proposals = Vec::new(&env);

        //Look through proposals using iter
        for i in 1..=proposal_count {
            if let Some(proposal) = env.storage().instance().get(&DataKey::Proposal(i)) {
                proposals.push_back(proposal);
            }
        }
        Ok(proposals)
    }

    //Voting on a proposed Proposal
    pub fn vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        is_yes: bool,
        amount: u64,
    ) -> Result<(), DAOError> {
        voter.require_auth();

        if amount <= 0 {
            return Err(DAOError::InvalidVote);
        }

        //get proposal
        let mut proposal: Proposal = env
            .storage()
            .instance()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(DAOError::InvalidProposal)?;

        //check if proposal is still active
        let current_time = env.ledger().timestamp();
        if current_time > proposal.deadline || !proposal.active {
            return Err(DAOError::ProposalExpired);
        }

        //Check if voter has already voted
        if env
            .storage()
            .instance()
            .has(&DataKey::Vote(proposal_id, voter.clone()))
        {
            return Err(DAOError::AlreadyVoted);
        }

        //get token ID and Client
        let token_config: TokenConfig = env
            .storage()
            .instance()
            .get(&DataKey::TokenConfig)
            .ok_or(DAOError::NotInitialised)?;
        let token_client = token::Client::new(&env, &token_config.admin);

        // Check if voter has sufficient tokens
        let available_balance: u64 = token_client
            .balance(&voter)
            .try_into()
            .map_err(|_| DAOError::InvalidVote)?;
        if available_balance < amount {
            return Err(DAOError::InsufficientTokens);
        }

        //Lock tokens by transferring to contract
        token_client.transfer(&voter, &env.current_contract_address(), &(amount as i128));

        //Record vote
        let vote = Vote {
            voter: voter.clone(),
            amount,
            is_yes,
        };
        env.storage()
            .instance()
            .set(&DataKey::Vote(proposal_id, voter.clone()), &vote);

        // Update proposal vote counts
        if is_yes {
            proposal.yes_votes += amount;
        } else {
            proposal.no_votes += amount;
        }
        env.storage()
            .instance()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        //Track locked tokens for voters
        let mut locked_amount: u64 = env
            .storage()
            .instance()
            .get(&DataKey::LockedTokens(voter.clone()))
            .unwrap_or(0);
        locked_amount += amount;
        env.storage()
            .instance()
            .set(&DataKey::LockedTokens(voter), &locked_amount);

        Ok(())
    }

    // Tally proposal outcome and close voting
    pub fn tally_proposal(env: Env, proposal_id: u64) -> Result<bool, DAOError> {
        let mut proposal: Proposal = env
            .storage()
            .instance()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(DAOError::InvalidProposal)?;

        // Check if voting period has ended
        let current_time = env.ledger().timestamp();
        if current_time <= proposal.deadline {
            return Err(DAOError::VotingNotClosed);
        }

        // Get total token supply
        let token_config: TokenConfig = env
            .storage()
            .instance()
            .get(&DataKey::TokenConfig)
            .ok_or(DAOError::NotInitialised)?;
        let total_supply = token_config.total_supply;

        // Calculate total votes
        let total_votes = proposal.yes_votes + proposal.no_votes;

        // Check quorum: total votes > 51% of total supply
        let quorum_threshold = total_supply * 51 / 100;
        if total_votes <= quorum_threshold {
            proposal.active = false;
            env.storage()
                .instance()
                .set(&DataKey::Proposal(proposal_id), &proposal);
            return Ok(false);
        }

        // Determine outcome: yes votes > no votes
        let passed = proposal.yes_votes > proposal.no_votes;

        // Close proposal
        proposal.active = false;
        env.storage()
            .instance()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        Ok(passed)
    }

    // Unlock tokens after voting deadline
    pub fn unlock_tokens(env: Env, voter: Address) -> Result<(), DAOError> {
        // Require voter authorization
        voter.require_auth();

        // Get locked token amount
        let locked_amount: u64 = env
            .storage()
            .instance()
            .get(&DataKey::LockedTokens(voter.clone()))
            .unwrap_or(0);
        if locked_amount == 0 {
            return Ok(());
        }

        // Get token ID
        let token_config: TokenConfig = env
            .storage()
            .instance()
            .get(&DataKey::TokenConfig)
            .ok_or(DAOError::NotInitialised)?;
        let token_client = token::Client::new(&env, &token_config.admin);

        // Transfer locked tokens back to voter
        token_client.transfer(
            &env.current_contract_address(),
            &voter,
            &(locked_amount as i128),
        );

        // Clear locked token record
        env.storage()
            .instance()
            .remove(&DataKey::LockedTokens(voter));

        Ok(())
    }
}

mod test;