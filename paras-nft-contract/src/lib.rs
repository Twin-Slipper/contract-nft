use near_contract_standards::non_fungible_token::core::{
    NonFungibleTokenCore, NonFungibleTokenResolver,
};
use near_contract_standards::non_fungible_token::metadata::{
    NFTContractMetadata, NonFungibleTokenMetadataProvider, TokenMetadata, NFT_METADATA_SPEC,
};
use near_contract_standards::non_fungible_token::NonFungibleToken;
use near_contract_standards::non_fungible_token::{Token, TokenId};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, UnorderedMap, UnorderedSet};
use near_sdk::env::is_valid_account_id;
use near_sdk::json_types::{ValidAccountId, U128, U64};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{
    assert_one_yocto, env, ext_contract, near_bindgen, serde_json::json, AccountId, Balance,
    BorshStorageKey, Gas, PanicOnDefault, Promise, PromiseOrValue, Timestamp,
};
use std::collections::HashMap;
use std::convert::TryInto;
use std::vec;

pub mod event;
pub use event::NearEvent;

mod raffle;
use raffle::Raffle;

/// between token_series_id and edition number e.g. 42:2 where 42 is series and 2 is edition
pub const TOKEN_DELIMETER: char = ':';
/// TokenMetadata.title returned for individual token e.g. "Title — 2/10" where 10 is max copies
pub const TITLE_DELIMETER: &str = " #";
/// e.g. "Title — 2/10" where 10 is max copies
pub const EDITION_DELIMETER: &str = "/";

const GAS_FOR_RESOLVE_TRANSFER: Gas = 10_000_000_000_000;
const GAS_FOR_NFT_TRANSFER_CALL: Gas = 30_000_000_000_000 + GAS_FOR_RESOLVE_TRANSFER;
const GAS_FOR_NFT_APPROVE: Gas = 10_000_000_000_000;
const GAS_FOR_MINT: Gas = 90_000_000_000_000;
const NO_DEPOSIT: Balance = 0;
const MAX_PRICE: Balance = 1_000_000_000 * 10u128.pow(24);

pub type TokenSeriesId = String;
pub type TimestampSec = u32;
pub type ContractAndTokenId = String;

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Payout {
    pub payout: HashMap<AccountId, U128>,
}

#[ext_contract(ext_non_fungible_token_receiver)]
trait NonFungibleTokenReceiver {
    /// Returns `true` if the token should be returned back to the sender.
    fn nft_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: TokenId,
        msg: String,
    ) -> Promise;
}

#[ext_contract(ext_approval_receiver)]
pub trait NonFungibleTokenReceiver {
    fn nft_on_approve(
        &mut self,
        token_id: TokenId,
        owner_id: AccountId,
        approval_id: u64,
        msg: String,
    );
}

#[ext_contract(ext_self)]
trait NonFungibleTokenResolver {
    fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
    ) -> bool;
}

#[ext_contract(ext_whitelist_contract)]
trait WhitelistContract {
    fn incress_balance_whitelist(&mut self, account_id: AccountId) -> u128;
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct TokenSeries {
    metadata: TokenMetadata,
    creator_id: AccountId,
    tokens: UnorderedSet<TokenId>,
    price: Option<Balance>,
    is_mintable: bool,
    royalty: HashMap<AccountId, u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct TokenSeriesJson {
    token_series_id: TokenSeriesId,
    metadata: TokenMetadata,
    creator_id: AccountId,
    royalty: HashMap<AccountId, u32>,
    transaction_fee: Option<U128>,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct TransactionFee {
    pub next_fee: Option<u16>,
    pub start_time: Option<TimestampSec>,
    pub current_fee: u16,
}

#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct MarketDataTransactionFee {
    pub transaction_fee: UnorderedMap<TokenSeriesId, u128>,
}

near_sdk::setup_alloc!();

#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct ContractV1 {
    tokens: NonFungibleToken,
    metadata: LazyOption<NFTContractMetadata>,
    // CUSTOM
    token_series_by_id: UnorderedMap<TokenSeriesId, TokenSeries>,
    treasury_id: AccountId,
    transaction_fee: TransactionFee,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    tokens: NonFungibleToken,
    metadata: LazyOption<NFTContractMetadata>,
    // CUSTOM
    token_series_by_id: UnorderedMap<TokenSeriesId, TokenSeries>,
    seller_by_id: UnorderedMap<AccountId, u128>,
    raffle: Raffle,
    token_series_id_minted: u128,
    treasury_id: AccountId,
    whitelist_contract_id: AccountId,
    transaction_fee: TransactionFee,
    account_id_og: HashMap<AccountId, u32>,
    balance_mint_og: u32,
    market_data_transaction_fee: MarketDataTransactionFee,
}

const DATA_IMAGE_SVG_PARAS_ICON: &str = "data:image/png;base64,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";

#[derive(BorshSerialize, BorshStorageKey)]
enum StorageKey {
    NonFungibleToken,
    Metadata,
    TokenMetadata,
    Enumeration,
    Approval,
    // CUSTOM
    TokenSeriesById,
    TokensBySeriesInner { token_series: String },
    TokensPerOwner { account_hash: Vec<u8> },
    MarketDataTransactionFee,
    SellerById,
    Raffle,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new_default_meta(
        owner_id: ValidAccountId,
        treasury_id: ValidAccountId,
        whitelist_contract_id: AccountId,
        max_supply_raffle: u32,
    ) -> Self {
        Self::new(
            owner_id,
            treasury_id,
            max_supply_raffle,
            NFTContractMetadata {
                spec: NFT_METADATA_SPEC.to_string(),
                name: "Fella Blooms".to_string(),
                symbol: "FELLA".to_string(),
                icon: Some(DATA_IMAGE_SVG_PARAS_ICON.to_string()),
                base_uri: Some("https://bafybeih2a6dl6au3qztlpjna3sdkfmuhfugcdg4blct7ahpre2qerf3pei.ipfs.nftstorage.link/".to_string()),
                reference: None,
                reference_hash: None,
            },
            whitelist_contract_id,
            500,
        )
    }

    #[init]
    pub fn new(
        owner_id: ValidAccountId,
        treasury_id: ValidAccountId,
        max_supply_raffle: u32,
        metadata: NFTContractMetadata,
        whitelist_contract_id: AccountId,
        current_fee: u16,
    ) -> Self {
        assert!(!env::state_exists(), "Already initialized");
        metadata.assert_valid();
        Self {
            tokens: NonFungibleToken::new(
                StorageKey::NonFungibleToken,
                owner_id,
                Some(StorageKey::TokenMetadata),
                Some(StorageKey::Enumeration),
                Some(StorageKey::Approval),
            ),
            token_series_by_id: UnorderedMap::new(StorageKey::TokenSeriesById),
            metadata: LazyOption::new(StorageKey::Metadata, Some(&metadata)),
            treasury_id: treasury_id.to_string(),
            seller_by_id: UnorderedMap::new(StorageKey::SellerById),
            raffle: Raffle::new(StorageKey::Raffle, max_supply_raffle as u64),
            token_series_id_minted: 0,
            whitelist_contract_id: whitelist_contract_id,
            account_id_og: HashMap::new(),
            balance_mint_og: 0,
            transaction_fee: TransactionFee {
                next_fee: None,
                start_time: None,
                current_fee,
            },
            market_data_transaction_fee: MarketDataTransactionFee {
                transaction_fee: UnorderedMap::new(StorageKey::MarketDataTransactionFee),
            },
        }
    }

    #[payable]
    pub fn set_transaction_fee(&mut self, next_fee: u16, start_time: Option<TimestampSec>) {
        assert_one_yocto();
        assert_eq!(
            env::predecessor_account_id(),
            self.tokens.owner_id,
            "Paras: Owner only"
        );

        assert!(
            next_fee < 10_000,
            "Paras: transaction fee is more than 10_000"
        );

        if start_time.is_none() {
            self.transaction_fee.current_fee = next_fee;
            self.transaction_fee.next_fee = None;
            self.transaction_fee.start_time = None;
            return;
        } else {
            let start_time: TimestampSec = start_time.unwrap();
            assert!(
                start_time > to_sec(env::block_timestamp()),
                "start_time is less than current block_timestamp"
            );
            self.transaction_fee.next_fee = Some(next_fee);
            self.transaction_fee.start_time = Some(start_time);
        }
    }

    pub fn calculate_market_data_transaction_fee(
        &mut self,
        token_series_id: &TokenSeriesId,
    ) -> u128 {
        if let Some(transaction_fee) = self
            .market_data_transaction_fee
            .transaction_fee
            .get(&token_series_id)
        {
            return transaction_fee;
        }

        // fallback to default transaction fee
        self.calculate_current_transaction_fee()
    }

    pub fn calculate_current_transaction_fee(&mut self) -> u128 {
        let transaction_fee: &TransactionFee = &self.transaction_fee;
        if transaction_fee.next_fee.is_some() {
            if to_sec(env::block_timestamp()) >= transaction_fee.start_time.unwrap() {
                self.transaction_fee.current_fee = transaction_fee.next_fee.unwrap();
                self.transaction_fee.next_fee = None;
                self.transaction_fee.start_time = None;
            }
        }
        self.transaction_fee.current_fee as u128
    }

    pub fn get_transaction_fee(&self) -> &TransactionFee {
        &self.transaction_fee
    }

    pub fn get_market_data_transaction_fee(&self, token_series_id: &TokenId) -> u128 {
        if let Some(transaction_fee) = self
            .market_data_transaction_fee
            .transaction_fee
            .get(&token_series_id)
        {
            return transaction_fee;
        }
        // fallback to default transaction fee
        self.transaction_fee.current_fee as u128
    }

    pub fn get_raffle_length(&self) -> u64 {
        return self.raffle.len();
    }

    pub fn get_balance_mint_og(&self) -> u32 {
        self.balance_mint_og
    }

    pub fn is_og(&self, account_id: &AccountId) -> bool {
        let contain = self.account_id_og.contains_key(account_id);
        return contain;
        // if contain {
        //     let balance = self.account_id_og.get(account_id).unwrap().clone();
        //     if balance > 0 {
        //         true
        //     } else {
        //         false
        //     }
        // } else {
        //     false
        // }
    }

    pub fn get_og_balance(&self, account_id: &AccountId) -> u32 {
        let contain = self.account_id_og.contains_key(account_id);

        if contain {
            self.account_id_og.get(account_id).unwrap().clone()
        } else {
            0
        }
    }

    #[payable]
    pub fn set_balance_mint_og(&mut self, balance_mint_og: u32) {
        assert_one_yocto();
        assert_eq!(
            env::predecessor_account_id(),
            self.tokens.owner_id,
            "Paras: Owner only"
        );
        self.balance_mint_og = balance_mint_og;
    }

    pub fn get_og_account_id(&self) -> HashMap<AccountId, u32> {
        return self.account_id_og.clone();
    }

    #[payable]
    pub fn add_og_account_id(&mut self, account_id: AccountId, balance_mint_og: Option<u32>) {
        assert_one_yocto();
        assert_eq!(
            env::predecessor_account_id(),
            self.tokens.owner_id,
            "Paras: Owner only"
        );
        let balance = if let Some(balance) = balance_mint_og {
            balance
        } else {
            self.balance_mint_og
        };

        self.account_id_og.insert(account_id, balance);
    }

    #[payable]
    pub fn decress_balance_og(&mut self, account_id: AccountId, balance_mint_og: u32) {
        self.account_id_og.insert(account_id, balance_mint_og - 1);
    }

    #[payable]
    pub fn remove_og_account_id(&mut self, account_id: AccountId) {
        assert_one_yocto();
        assert_eq!(
            env::predecessor_account_id(),
            self.tokens.owner_id,
            "Paras: Owner only"
        );
        self.account_id_og.remove(&account_id);
    }

    // Treasury
    #[payable]
    pub fn set_treasury(&mut self, treasury_id: ValidAccountId) {
        assert_one_yocto();
        assert_eq!(
            env::predecessor_account_id(),
            self.tokens.owner_id,
            "Paras: Owner only"
        );
        self.treasury_id = treasury_id.to_string();
    }

    // CUSTOM

    #[payable]
    pub fn nft_create_series(
        &mut self,
        creator_id: Option<ValidAccountId>,
        token_metadata: TokenMetadata,
        price: Option<U128>,
        royalty: Option<HashMap<AccountId, u32>>,
    ) -> TokenSeriesJson {
        let initial_storage_usage = env::storage_usage();
        let caller_id = env::predecessor_account_id();

        assert_eq!(caller_id, self.tokens.owner_id, "Paras: Only owner");

        if creator_id.is_some() {
            assert_eq!(
                creator_id.unwrap().to_string(),
                caller_id,
                "Paras: Caller is not creator_id"
            );
        }

        let token_series_id = format!("{}", (self.token_series_by_id.len() + 1));

        assert!(
            self.token_series_by_id.get(&token_series_id).is_none(),
            "Paras: duplicate token_series_id"
        );

        let title = token_metadata.title.clone();
        assert!(title.is_some(), "Paras: token_metadata.title is required");

        let mut total_perpetual = 0;
        let mut total_accounts = 0;
        let royalty_res: HashMap<AccountId, u32> = if let Some(royalty) = royalty {
            for (k, v) in royalty.iter() {
                if !is_valid_account_id(k.as_bytes()) {
                    env::panic("Not valid account_id for royalty".as_bytes());
                };
                total_perpetual += *v;
                total_accounts += 1;
            }
            royalty 
        } else {
            HashMap::new()
        };

        assert!(total_accounts <= 10, "Paras: royalty exceeds 10 accounts");

        assert!(
            total_perpetual <= 9000,
            "Paras Exceeds maximum royalty -> 9000",
        );

        let price_res: Option<u128> = if price.is_some() {
            assert!(
                price.unwrap().0 < MAX_PRICE,
                "Paras: price higher than {}",
                MAX_PRICE
            );
            Some(price.unwrap().0)
        } else {
            None
        };

        self.token_series_by_id.insert(
            &token_series_id,
            &TokenSeries {
                metadata: token_metadata.clone(),
                creator_id: caller_id.to_string(),
                tokens: UnorderedSet::new(
                    StorageKey::TokensBySeriesInner {
                        token_series: token_series_id.clone(),
                    }
                    .try_to_vec()
                    .unwrap(),
                ),
                price: price_res,
                is_mintable: true,
                royalty: royalty_res.clone(),
            },
        );

        // set market data transaction fee
        let current_transaction_fee = self.calculate_current_transaction_fee();
        self.market_data_transaction_fee
            .transaction_fee
            .insert(&token_series_id, &current_transaction_fee);

        env::log(
            json!({
                "type": "nft_create_series",
                "params": {
                    "token_series_id": token_series_id,
                    "token_metadata": token_metadata,
                    "creator_id": caller_id,
                    "price": price,
                    "royalty": royalty_res,
                    "transaction_fee": &current_transaction_fee.to_string()
                }
            })
            .to_string()
            .as_bytes(),
        );

        refund_deposit(env::storage_usage() - initial_storage_usage, 0);

        TokenSeriesJson {
            token_series_id,
            metadata: token_metadata,
            creator_id: caller_id.into(),
            royalty: royalty_res,
            transaction_fee: Some(current_transaction_fee.into()),
        }
    }

    #[payable]
    pub fn nft_create_series_custom(
        &mut self,
        token_series_id: TokenSeriesId,
        creator_id: Option<ValidAccountId>,
        token_metadata: TokenMetadata,
        price: Option<U128>,
        royalty: Option<HashMap<AccountId, u32>>,
    ) -> TokenSeriesJson {
        let initial_storage_usage = env::storage_usage();
        let caller_id = env::predecessor_account_id();

        assert_eq!(caller_id, self.tokens.owner_id, "Paras: Only owner");

        if creator_id.is_some() {
            assert_eq!(
                creator_id.unwrap().to_string(),
                caller_id,
                "Paras: Caller is not creator_id"
            );
        }

        assert!(
            self.token_series_by_id.get(&token_series_id).is_none(),
            "Paras: duplicate token_series_id"
        );

        let title = token_metadata.title.clone();
        assert!(title.is_some(), "Paras: token_metadata.title is required");

        let mut total_perpetual = 0;
        let mut total_accounts = 0;
        let royalty_res: HashMap<AccountId, u32> = if let Some(royalty) = royalty {
            for (k, v) in royalty.iter() {
                if !is_valid_account_id(k.as_bytes()) {
                    env::panic("Not valid account_id for royalty".as_bytes());
                };
                total_perpetual += *v;
                total_accounts += 1;
            }
            royalty
        } else {
            HashMap::new()
        };

        assert!(total_accounts <= 10, "Paras: royalty exceeds 10 accounts");

        assert!(
            total_perpetual <= 9000,
            "Paras Exceeds maximum royalty -> 9000",
        );

        let price_res: Option<u128> = if price.is_some() {
            assert!(
                price.unwrap().0 < MAX_PRICE,
                "Paras: price higher than {}",
                MAX_PRICE
            );
            Some(price.unwrap().0)
        } else {
            None
        };

        self.token_series_by_id.insert(
            &token_series_id,
            &TokenSeries {
                metadata: token_metadata.clone(),
                creator_id: caller_id.to_string(),
                tokens: UnorderedSet::new(
                    StorageKey::TokensBySeriesInner {
                        token_series: token_series_id.clone(),
                    }
                    .try_to_vec()
                    .unwrap(),
                ),
                price: price_res,
                is_mintable: true,
                royalty: royalty_res.clone(),
            },
        );

        // set market data transaction fee
        let current_transaction_fee = self.calculate_current_transaction_fee();
        self.market_data_transaction_fee
            .transaction_fee
            .insert(&token_series_id, &current_transaction_fee);

        env::log(
            json!({
                "type": "nft_create_series",
                "params": {
                    "token_series_id": token_series_id,
                    "token_metadata": token_metadata,
                    "creator_id": caller_id,
                    "price": price,
                    "royalty": royalty_res,
                    "transaction_fee": &current_transaction_fee.to_string()
                }
            })
            .to_string()
            .as_bytes(),
        );

        refund_deposit(env::storage_usage() - initial_storage_usage, 0);

        TokenSeriesJson {
            token_series_id,
            metadata: token_metadata,
            creator_id: caller_id.into(),
            royalty: royalty_res,
            transaction_fee: Some(current_transaction_fee.into()),
        }
    }

    #[payable]
    pub fn nft_mint_creator(
        &mut self,
        token_series_id: TokenSeriesId,
        receiver_id: ValidAccountId,
    ) -> TokenId {
        let initial_storage_usage = env::storage_usage();

        let token_series = self
            .token_series_by_id
            .get(&token_series_id)
            .expect("Paras: Token series not exist");
        assert_eq!(
            env::predecessor_account_id(),
            token_series.creator_id,
            "Paras: not creator"
        );
        let token_id: TokenId = self._nft_mint_series(token_series_id, receiver_id.to_string());

        refund_deposit(env::storage_usage() - initial_storage_usage, 0);

        NearEvent::log_nft_mint(receiver_id.to_string(), vec![token_id.clone()], None);

        token_id
    }

    //draw a token from a token series
    #[payable]
    pub fn draw_and_mint(&mut self, receiver_id: ValidAccountId) -> TokenId {
        let initial_storage_usage = env::storage_usage();
        let caller = env::predecessor_account_id();
        // log(token_series_id.as_bytes());

        let account_id: ValidAccountId = caller.clone().try_into().unwrap();

        if self.get_raffle_length() == 0 {
            panic!("Sold Out")
        }

        if !self.is_og(&caller) {
            let tokens_per_owner = self.tokens.tokens_per_owner.as_ref().expect(
                "Could not find tokens_per_owner when calling a method on the enumeration standard.",
            );

            let token_set = tokens_per_owner.get(account_id.as_ref());
            if !token_set.is_none() {
                let token_tmp = token_set.unwrap();
                if token_tmp.len() >= 1 {
                    panic!("Mint Not Allowed")
                }
            }
        } else {
            let balance = self.account_id_og.get(&caller).unwrap().clone();
            if balance < 1 {
                panic!("Mint Not Allowed")
            }
        }

        let token_series_id = (self.raffle.draw() + 1).to_string(); //random token series id from 1 to max size

        self.token_series_by_id
            .get(&token_series_id)
            .expect("Paras: Token series not exist");
        // let token_series = self.token_series_by_id.get(&token_series_id).expect("Paras: Token series not exist");
        // assert_eq!(env::predecessor_account_id(), token_series.creator_id, "Paras: not creator");
        let token_id: TokenId = self._nft_mint_series(token_series_id, receiver_id.to_string());

        if self.is_og(&caller) {
            let balance = self.get_og_balance(&caller);
            self.decress_balance_og(caller.clone(), balance);
        }

        // ext_whitelist_contract::incress_balance_whitelist(
        //     caller.clone(),
        //     &self.whitelist_contract_id.clone(),
        //     NO_DEPOSIT,
        //     GAS_FOR_RESOLVE_TRANSFER,
        // );

        refund_deposit(env::storage_usage() - initial_storage_usage, 0);

        NearEvent::log_nft_mint(receiver_id.to_string(), vec![token_id.clone()], None);

        token_id
    }

    //custom mint token series
    #[payable]
    pub fn nft_mint(
        &mut self,
        token_series_id: TokenSeriesId,
        receiver_id: ValidAccountId,
    ) -> TokenId {
        let caller = env::predecessor_account_id();
        let initial_storage_usage = env::storage_usage();
        // log(token_series_id.as_bytes());
        let contain = self.account_id_og.contains_key(&caller);
        assert!(contain, "Not in OG list");

        let balance_og = self.account_id_og.get(&caller).unwrap().clone();
        if balance_og <= 0 {
            panic!("Not enough balance in OG");
        }

        self.token_series_by_id
            .get(&token_series_id)
            .expect("Paras: Token series not exist");
        // let token_series = self.token_series_by_id.get(&token_series_id).expect("Paras: Token series not exist");
        // assert_eq!(env::predecessor_account_id(), token_series.creator_id, "Paras: not creator");
        let token_id: TokenId = self._nft_mint_series(token_series_id, receiver_id.to_string());

        //decrease balance in OG
        self.decress_balance_og(caller.clone(), balance_og);

        refund_deposit(env::storage_usage() - initial_storage_usage, 0);

        NearEvent::log_nft_mint(receiver_id.to_string(), vec![token_id.clone()], None);

        token_id
    }

    #[payable]
    pub fn nft_mint_and_approve(
        &mut self,
        token_series_id: TokenSeriesId,
        account_id: ValidAccountId,
        msg: Option<String>,
    ) -> Option<Promise> {
        let initial_storage_usage = env::storage_usage();

        let token_series = self
            .token_series_by_id
            .get(&token_series_id)
            .expect("Paras: Token series not exist");
        assert_eq!(
            env::predecessor_account_id(),
            token_series.creator_id,
            "Paras: not creator"
        );
        let token_id: TokenId =
            self._nft_mint_series(token_series_id, token_series.creator_id.clone());

        // Need to copy the nft_approve code here to solve the gas problem
        // get contract-level LookupMap of token_id to approvals HashMap
        let approvals_by_id = self.tokens.approvals_by_id.as_mut().unwrap();

        // update HashMap of approvals for this token
        let approved_account_ids = &mut approvals_by_id
            .get(&token_id)
            .unwrap_or_else(|| HashMap::new());
        let account_id: AccountId = account_id.into();
        let approval_id: u64 = self
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&token_id)
            .unwrap_or_else(|| 1u64);
        approved_account_ids.insert(account_id.clone(), approval_id);

        // save updated approvals HashMap to contract's LookupMap
        approvals_by_id.insert(&token_id, &approved_account_ids);

        // increment next_approval_id for this token
        self.tokens
            .next_approval_id_by_id
            .as_mut()
            .unwrap()
            .insert(&token_id, &(approval_id + 1));

        refund_deposit(env::storage_usage() - initial_storage_usage, 0);

        NearEvent::log_nft_mint(
            token_series.creator_id.clone(),
            vec![token_id.clone()],
            None,
        );

        if let Some(msg) = msg {
            Some(ext_approval_receiver::nft_on_approve(
                token_id,
                token_series.creator_id,
                approval_id,
                msg,
                &account_id,
                NO_DEPOSIT,
                env::prepaid_gas() - GAS_FOR_NFT_APPROVE - GAS_FOR_MINT,
            ))
        } else {
            None
        }
    }

    fn _nft_mint_series(
        &mut self,
        token_series_id: TokenSeriesId,
        receiver_id: AccountId,
    ) -> TokenId {
        let mut token_series = self
            .token_series_by_id
            .get(&token_series_id)
            .expect("Paras: Token series not exist");
        assert!(
            token_series.is_mintable,
            "Paras: Token series is not mintable"
        );

        let num_tokens = token_series.tokens.len();
        let max_copies = token_series.metadata.copies.unwrap_or(u64::MAX);
        assert!(num_tokens < max_copies, "Series supply maxed");

        if (num_tokens + 1) >= max_copies {
            token_series.is_mintable = false;
        }

        let token_id = format!("{}{}{}", &token_series_id, TOKEN_DELIMETER, num_tokens + 1);
        token_series.tokens.insert(&token_id);
        self.token_series_by_id
            .insert(&token_series_id, &token_series);

        // you can add custom metadata to each token here
        let metadata = Some(TokenMetadata {
            title: None,       // ex. "Arch Nemesis: Mail Carrier" or "Parcel #5055"
            description: None, // free-form description
            media: None, // URL to associated media, preferably to decentralized, content-addressed storage
            media_hash: None, // Base64-encoded sha256 hash of content referenced by the `media` field. Required if `media` is included.
            copies: None, // number of copies of this set of metadata in existence when token was minted.
            issued_at: Some(env::block_timestamp().to_string()), // ISO 8601 datetime when token was issued or minted
            expires_at: None,     // ISO 8601 datetime when token expires
            starts_at: None,      // ISO 8601 datetime when token starts being valid
            updated_at: None,     // ISO 8601 datetime when token was last updated
            extra: None, // anything extra the NFT wants to store on-chain. Can be stringified JSON.
            reference: None, // URL to an off-chain JSON file with more info.
            reference_hash: None, // Base64-encoded sha256 hash of JSON from reference field. Required if `reference` is included.
        });

        //let token = self.tokens.mint(token_id, receiver_id, metadata);
        // From : https://github.com/near/near-sdk-rs/blob/master/near-contract-standards/src/non_fungible_token/core/core_impl.rs#L359
        // This allows lazy minting

        let owner_id: AccountId = receiver_id;
        self.tokens.owner_by_id.insert(&token_id, &owner_id);

        self.tokens
            .token_metadata_by_id
            .as_mut()
            .and_then(|by_id| by_id.insert(&token_id, &metadata.as_ref().unwrap()));

        if let Some(tokens_per_owner) = &mut self.tokens.tokens_per_owner {
            let mut token_ids = tokens_per_owner.get(&owner_id).unwrap_or_else(|| {
                UnorderedSet::new(StorageKey::TokensPerOwner {
                    account_hash: env::sha256(&owner_id.as_bytes()),
                })
            });
            token_ids.insert(&token_id);
            tokens_per_owner.insert(&owner_id, &token_ids);
        }

        self.token_series_id_minted = self.token_series_id_minted + 1;

        //cross contract call to update balance whitelist

        token_id
    }

    #[payable]
    pub fn nft_set_series_non_mintable(&mut self, token_series_id: TokenSeriesId) {
        assert_one_yocto();

        let mut token_series = self
            .token_series_by_id
            .get(&token_series_id)
            .expect("Token series not exist");
        assert_eq!(
            env::predecessor_account_id(),
            token_series.creator_id,
            "Paras: Creator only"
        );

        assert_eq!(
            token_series.is_mintable, true,
            "Paras: already non-mintable"
        );

        assert_eq!(
            token_series.metadata.copies, None,
            "Paras: decrease supply if copies not null"
        );

        token_series.is_mintable = false;
        self.token_series_by_id
            .insert(&token_series_id, &token_series);
        env::log(
            json!({
                "type": "nft_set_series_non_mintable",
                "params": {
                    "token_series_id": token_series_id,
                }
            })
            .to_string()
            .as_bytes(),
        );
    }

    #[payable]
    pub fn nft_decrease_series_copies(
        &mut self,
        token_series_id: TokenSeriesId,
        decrease_copies: U64,
    ) -> U64 {
        assert_one_yocto();

        let mut token_series = self
            .token_series_by_id
            .get(&token_series_id)
            .expect("Token series not exist");
        assert_eq!(
            env::predecessor_account_id(),
            token_series.creator_id,
            "Paras: Creator only"
        );

        let minted_copies = token_series.tokens.len();
        let copies = token_series.metadata.copies.unwrap();

        assert!(
            (copies - decrease_copies.0) >= minted_copies,
            "Paras: cannot decrease supply, already minted : {}",
            minted_copies
        );

        let is_non_mintable = if (copies - decrease_copies.0) == minted_copies {
            token_series.is_mintable = false;
            true
        } else {
            false
        };

        token_series.metadata.copies = Some(copies - decrease_copies.0);

        self.token_series_by_id
            .insert(&token_series_id, &token_series);
        env::log(
            json!({
                "type": "nft_decrease_series_copies",
                "params": {
                    "token_series_id": token_series_id,
                    "copies": U64::from(token_series.metadata.copies.unwrap()),
                    "is_non_mintable": is_non_mintable,
                }
            })
            .to_string()
            .as_bytes(),
        );
        U64::from(token_series.metadata.copies.unwrap())
    }

    #[payable]
    pub fn nft_set_series_price(
        &mut self,
        token_series_id: TokenSeriesId,
        price: Option<U128>,
    ) -> Option<U128> {
        assert_one_yocto();

        let mut token_series = self
            .token_series_by_id
            .get(&token_series_id)
            .expect("Token series not exist");
        assert_eq!(
            env::predecessor_account_id(),
            token_series.creator_id,
            "Paras: Creator only"
        );

        assert_eq!(
            token_series.is_mintable, true,
            "Paras: token series is not mintable"
        );

        if price.is_none() {
            token_series.price = None;
        } else {
            assert!(
                price.unwrap().0 < MAX_PRICE,
                "Paras: price higher than {}",
                MAX_PRICE
            );
            token_series.price = Some(price.unwrap().0);
        }

        self.token_series_by_id
            .insert(&token_series_id, &token_series);

        // set market data transaction fee
        let current_transaction_fee = self.calculate_current_transaction_fee();
        self.market_data_transaction_fee
            .transaction_fee
            .insert(&token_series_id, &current_transaction_fee);

        env::log(
            json!({
                "type": "nft_set_series_price",
                "params": {
                    "token_series_id": token_series_id,
                    "price": price,
                    "transaction_fee": current_transaction_fee.to_string()
                }
            })
            .to_string()
            .as_bytes(),
        );
        return price;
    }

    #[payable]
    pub fn nft_burn(&mut self, token_id: TokenId) {
        assert_one_yocto();

        let owner_id = self.tokens.owner_by_id.get(&token_id).unwrap();
        assert_eq!(owner_id, env::predecessor_account_id(), "Token owner only");

        if let Some(next_approval_id_by_id) = &mut self.tokens.next_approval_id_by_id {
            next_approval_id_by_id.remove(&token_id);
        }

        if let Some(approvals_by_id) = &mut self.tokens.approvals_by_id {
            approvals_by_id.remove(&token_id);
        }

        if let Some(tokens_per_owner) = &mut self.tokens.tokens_per_owner {
            let mut token_ids = tokens_per_owner.get(&owner_id).unwrap();
            token_ids.remove(&token_id);
            tokens_per_owner.insert(&owner_id, &token_ids);
        }

        if let Some(token_metadata_by_id) = &mut self.tokens.token_metadata_by_id {
            token_metadata_by_id.remove(&token_id);
        }

        self.tokens.owner_by_id.remove(&token_id);

        NearEvent::log_nft_burn(owner_id, vec![token_id], None, None);
    }

    // CUSTOM VIEWS

    pub fn nft_get_series_single(&self, token_series_id: TokenSeriesId) -> TokenSeriesJson {
        let token_series = self
            .token_series_by_id
            .get(&token_series_id)
            .expect("Series does not exist");
        let current_transaction_fee = self.get_market_data_transaction_fee(&token_series_id);
        TokenSeriesJson {
            token_series_id,
            metadata: token_series.metadata,
            creator_id: token_series.creator_id,
            royalty: token_series.royalty,
            transaction_fee: Some(current_transaction_fee.into()),
        }
    }

    pub fn nft_get_series_format(self) -> (char, &'static str, &'static str) {
        (TOKEN_DELIMETER, TITLE_DELIMETER, EDITION_DELIMETER)
    }

    pub fn nft_get_series_price(self, token_series_id: TokenSeriesId) -> Option<U128> {
        let price = self.token_series_by_id.get(&token_series_id).unwrap().price;
        match price {
            Some(p) => return Some(U128::from(p)),
            None => return None,
        };
    }

    pub fn nft_get_series(
        &self,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<TokenSeriesJson> {
        let start_index: u128 = from_index.map(From::from).unwrap_or_default();
        assert!(
            (self.token_series_by_id.len() as u128) > start_index,
            "Out of bounds, please use a smaller from_index."
        );
        let limit = limit.map(|v| v as usize).unwrap_or(usize::MAX);
        assert_ne!(limit, 0, "Cannot provide limit of 0.");

        self.token_series_by_id
            .iter()
            .skip(start_index as usize)
            .take(limit)
            .map(|(token_series_id, token_series)| TokenSeriesJson {
                token_series_id,
                metadata: token_series.metadata,
                creator_id: token_series.creator_id,
                royalty: token_series.royalty,
                transaction_fee: None,
            })
            .collect()
    }

    pub fn nft_supply_for_series(&self, token_series_id: TokenSeriesId) -> U64 {
        self.token_series_by_id
            .get(&token_series_id)
            .expect("Token series not exist")
            .tokens
            .len()
            .into()
    }

    pub fn nft_tokens_by_series(
        &self,
        token_series_id: TokenSeriesId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<Token> {
        let start_index: u128 = from_index.map(From::from).unwrap_or_default();
        let tokens = self
            .token_series_by_id
            .get(&token_series_id)
            .unwrap()
            .tokens;
        assert!(
            (tokens.len() as u128) > start_index,
            "Out of bounds, please use a smaller from_index."
        );
        let limit = limit.map(|v| v as usize).unwrap_or(usize::MAX);
        assert_ne!(limit, 0, "Cannot provide limit of 0.");

        tokens
            .iter()
            .skip(start_index as usize)
            .take(limit)
            .map(|token_id| self.nft_token(token_id).unwrap())
            .collect()
    }

    pub fn nft_token(&self, token_id: TokenId) -> Option<Token> {
        let owner_id = self.tokens.owner_by_id.get(&token_id)?;
        let approved_account_ids = self
            .tokens
            .approvals_by_id
            .as_ref()
            .and_then(|by_id| by_id.get(&token_id).or_else(|| Some(HashMap::new())));

        // CUSTOM (switch metadata for the token_series metadata)
        let mut token_id_iter = token_id.split(TOKEN_DELIMETER);
        let token_series_id = token_id_iter.next().unwrap().parse().unwrap();
        let series_metadata = self
            .token_series_by_id
            .get(&token_series_id)
            .unwrap()
            .metadata;

        let mut token_metadata = self
            .tokens
            .token_metadata_by_id
            .as_ref()
            .unwrap()
            .get(&token_id)
            .unwrap();

        // token_metadata.title = Some(format!(
        //     "{}{}{}",
        //     series_metadata.title.unwrap(),
        //     TITLE_DELIMETER,
        //     token_id_iter.next().unwrap()
        // ));

        token_metadata.title = Some(format!("{}", series_metadata.title.unwrap(),));

        token_metadata.reference = series_metadata.reference;
        token_metadata.media = series_metadata.media;
        token_metadata.copies = series_metadata.copies;
        token_metadata.extra = series_metadata.extra;

        Some(Token {
            token_id,
            owner_id,
            metadata: Some(token_metadata),
            approved_account_ids,
        })
    }

    pub fn is_seller(&self, account_id: AccountId) -> bool {
        let count_sell = if let Some(count) = self.seller_by_id.get(&account_id) {
            count
        } else {
            0
        };

        count_sell > 0
    }

    pub fn get_seller(&self) -> Vec<String> {
        let mut tmp: Vec<String> = vec![];
        for i in self.seller_by_id.keys() {
            tmp.push(i);
        }
        tmp
    }

    // CUSTOM core standard repeated here because no macro below

    pub fn nft_transfer_unsafe(
        &mut self,
        receiver_id: ValidAccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        let sender_id = env::predecessor_account_id();
        let receiver_id_str = receiver_id.to_string();
        let (previous_owner_id, _) = self.tokens.internal_transfer(
            &sender_id,
            &receiver_id_str,
            &token_id,
            approval_id,
            memo.clone(),
        );

        let authorized_id: Option<AccountId> = if sender_id != previous_owner_id {
            Some(sender_id)
        } else {
            None
        };

        NearEvent::log_nft_transfer(
            previous_owner_id,
            receiver_id_str,
            vec![token_id],
            memo,
            authorized_id,
        );
    }

    #[payable]
    pub fn nft_transfer(
        &mut self,
        receiver_id: ValidAccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        let sender_id = env::predecessor_account_id();
        let previous_owner_id = self
            .tokens
            .owner_by_id
            .get(&token_id)
            .expect("Token not found");
        let receiver_id_str = receiver_id.to_string();
        self.tokens
            .nft_transfer(receiver_id, token_id.clone(), approval_id, memo.clone());

        let authorized_id: Option<AccountId> = if sender_id != previous_owner_id {
            Some(sender_id)
        } else {
            None
        };

        NearEvent::log_nft_transfer(
            previous_owner_id,
            receiver_id_str,
            vec![token_id],
            memo,
            authorized_id,
        );
    }

    #[payable]
    pub fn assign_garden(
        &mut self,
        receiver_id: ValidAccountId,
        token_id: TokenId,
    ) -> PromiseOrValue<bool> {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();

        if !self.is_seller(sender_id.clone()) {
            panic!("You must sell at least 1 NFT")
        }

        let (previous_owner_id, old_approvals) =
            self.tokens
                .internal_transfer(&sender_id, receiver_id.as_ref(), &token_id, None, None);

        let authorized_id: Option<AccountId> = if sender_id != previous_owner_id {
            Some(sender_id.clone())
        } else {
            None
        };

        NearEvent::log_nft_transfer(
            previous_owner_id.clone(),
            receiver_id.to_string(),
            vec![token_id.clone()],
            None,
            authorized_id,
        );

        let msg = if self.is_seller(previous_owner_id.clone()) {
            "seller".to_string()
        } else {
            "not_seller".to_string()
        };

        // Initiating receiver's call and the callback
        ext_non_fungible_token_receiver::nft_on_transfer(
            sender_id,
            previous_owner_id.clone(),
            token_id.clone(),
            msg,
            receiver_id.as_ref(),
            NO_DEPOSIT,
            env::prepaid_gas() - GAS_FOR_NFT_TRANSFER_CALL,
        )
        .then(ext_self::nft_resolve_transfer(
            previous_owner_id,
            receiver_id.into(),
            token_id,
            old_approvals,
            &env::current_account_id(),
            NO_DEPOSIT,
            GAS_FOR_RESOLVE_TRANSFER,
        ))
        .into()
    }

    #[payable]
    pub fn nft_transfer_call(
        &mut self,
        receiver_id: ValidAccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        let (previous_owner_id, old_approvals) = self.tokens.internal_transfer(
            &sender_id,
            receiver_id.as_ref(),
            &token_id,
            approval_id,
            memo.clone(),
        );

        let authorized_id: Option<AccountId> = if sender_id != previous_owner_id {
            Some(sender_id.clone())
        } else {
            None
        };

        NearEvent::log_nft_transfer(
            previous_owner_id.clone(),
            receiver_id.to_string(),
            vec![token_id.clone()],
            memo,
            authorized_id,
        );

        // Initiating receiver's call and the callback
        ext_non_fungible_token_receiver::nft_on_transfer(
            sender_id,
            previous_owner_id.clone(),
            token_id.clone(),
            msg,
            receiver_id.as_ref(),
            NO_DEPOSIT,
            env::prepaid_gas() - GAS_FOR_NFT_TRANSFER_CALL,
        )
        .then(ext_self::nft_resolve_transfer(
            previous_owner_id,
            receiver_id.into(),
            token_id,
            old_approvals,
            &env::current_account_id(),
            NO_DEPOSIT,
            GAS_FOR_RESOLVE_TRANSFER,
        ))
        .into()
    }

    // CUSTOM enumeration standard modified here because no macro below

    pub fn nft_total_supply(&self) -> U128 {
        (self.tokens.owner_by_id.len() as u128).into()
    }

    pub fn nft_total_series_supply(&self) -> U128 {
        (self.token_series_by_id.len() as u128).into()
    }

    pub fn nft_total_series_minted(&self) -> U128 {
        (self.token_series_id_minted as u128).into()
    }

    pub fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<Token> {
        // Get starting index, whether or not it was explicitly given.
        // Defaults to 0 based on the spec:
        // https://nomicon.io/Standards/NonFungibleToken/Enumeration.html#interface
        let start_index: u128 = from_index.map(From::from).unwrap_or_default();
        assert!(
            (self.tokens.owner_by_id.len() as u128) > start_index,
            "Out of bounds, please use a smaller from_index."
        );
        let limit = limit.map(|v| v as usize).unwrap_or(usize::MAX);
        assert_ne!(limit, 0, "Cannot provide limit of 0.");
        self.tokens
            .owner_by_id
            .iter()
            .skip(start_index as usize)
            .take(limit)
            .map(|(token_id, _)| self.nft_token(token_id).unwrap())
            .collect()
    }

    pub fn nft_supply_for_owner(self, account_id: ValidAccountId) -> U128 {
        let tokens_per_owner = self.tokens.tokens_per_owner.expect(
            "Could not find tokens_per_owner when calling a method on the enumeration standard.",
        );
        tokens_per_owner
            .get(account_id.as_ref())
            .map(|account_tokens| U128::from(account_tokens.len() as u128))
            .unwrap_or(U128(0))
    }

    pub fn nft_tokens_for_owner(
        &self,
        account_id: ValidAccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<Token> {
        let tokens_per_owner = self.tokens.tokens_per_owner.as_ref().expect(
            "Could not find tokens_per_owner when calling a method on the enumeration standard.",
        );
        let token_set = if let Some(token_set) = tokens_per_owner.get(account_id.as_ref()) {
            token_set
        } else {
            return vec![];
        };
        let limit = limit.map(|v| v as usize).unwrap_or(usize::MAX);
        assert_ne!(limit, 0, "Cannot provide limit of 0.");
        let start_index: u128 = from_index.map(From::from).unwrap_or_default();
        assert!(
            token_set.len() as u128 > start_index,
            "Out of bounds, please use a smaller from_index."
        );
        token_set
            .iter()
            .skip(start_index as usize)
            .take(limit)
            .map(|token_id| self.nft_token(token_id).unwrap())
            .collect()
    }

    pub fn nft_payout(&self, token_id: TokenId, balance: U128, max_len_payout: u32) -> Payout {
        let owner_id = self.tokens.owner_by_id.get(&token_id).expect("No token id");
        let mut token_id_iter = token_id.split(TOKEN_DELIMETER);
        let token_series_id = token_id_iter.next().unwrap().parse().unwrap();
        let royalty = self
            .token_series_by_id
            .get(&token_series_id)
            .expect("no type")
            .royalty;

        assert!(
            royalty.len() as u32 <= max_len_payout,
            "Market cannot payout to that many receivers"
        );

        let balance_u128: u128 = balance.into();

        let mut payout: Payout = Payout {
            payout: HashMap::new(),
        };
        let mut total_perpetual = 0;

        for (k, v) in royalty.iter() {
            if *k != owner_id {
                let key = k.clone();
                payout
                    .payout
                    .insert(key, royalty_to_payout(*v, balance_u128));
                total_perpetual += *v;
            }
        }
        payout.payout.insert(
            owner_id,
            royalty_to_payout(10000 - total_perpetual, balance_u128),
        );
        payout
    }

    #[payable]
    pub fn nft_transfer_payout(
        &mut self,
        receiver_id: ValidAccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        balance: Option<U128>,
        max_len_payout: Option<u32>,
    ) -> Option<Payout> {
        assert_one_yocto();

        let sender_id = env::predecessor_account_id();
        // Transfer
        let previous_token = self.nft_token(token_id.clone()).expect("no token");
        self.tokens
            .nft_transfer(receiver_id.clone(), token_id.clone(), approval_id, None);

        // Payout calculation
        let previous_owner_id = previous_token.owner_id;
        let mut total_perpetual = 0;
        let payout = if let Some(balance) = balance {
            let balance_u128: u128 = u128::from(balance);
            let mut payout: Payout = Payout {
                payout: HashMap::new(),
            };

            let mut token_id_iter = token_id.split(TOKEN_DELIMETER);
            let token_series_id = token_id_iter.next().unwrap().parse().unwrap();
            let royalty = self
                .token_series_by_id
                .get(&token_series_id)
                .expect("no type")
                .royalty;

            assert!(
                royalty.len() as u32 <= max_len_payout.unwrap(),
                "Market cannot payout to that many receivers"
            );
            for (k, v) in royalty.iter() {
                let key = k.clone();
                if key != previous_owner_id {
                    payout
                        .payout
                        .insert(key, royalty_to_payout(*v, balance_u128));
                    total_perpetual += *v;
                }
            }

            assert!(total_perpetual <= 10000, "Total payout overflow");

            payout.payout.insert(
                previous_owner_id.clone(),
                royalty_to_payout(10000 - total_perpetual, balance_u128),
            );
            Some(payout)
        } else {
            None
        };

        let authorized_id: Option<AccountId> = if sender_id != previous_owner_id {
            Some(sender_id)
        } else {
            None
        };

        //insert seller to seller_by_id
        let count_sell = if let Some(count) = self.seller_by_id.get(&previous_owner_id) {
            count + 1
        } else {
            1
        };

        self.seller_by_id.insert(&previous_owner_id, &count_sell);

        NearEvent::log_nft_transfer(
            previous_owner_id,
            receiver_id.to_string(),
            vec![token_id],
            None,
            authorized_id,
        );

        payout
    }

    pub fn get_owner(&self) -> AccountId {
        self.tokens.owner_id.clone()
    }
}

fn royalty_to_payout(a: u32, b: Balance) -> U128 {
    U128(a as u128 * b / 10_000u128)
}

// near_contract_standards::impl_non_fungible_token_core!(Contract, tokens);
// near_contract_standards::impl_non_fungible_token_enumeration!(Contract, tokens);
near_contract_standards::impl_non_fungible_token_approval!(Contract, tokens);

#[near_bindgen]
impl NonFungibleTokenMetadataProvider for Contract {
    fn nft_metadata(&self) -> NFTContractMetadata {
        self.metadata.get().unwrap()
    }
}

#[near_bindgen]
impl NonFungibleTokenResolver for Contract {
    #[private]
    fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
    ) -> bool {
        let resp: bool = self.tokens.nft_resolve_transfer(
            previous_owner_id.clone(),
            receiver_id.clone(),
            token_id.clone(),
            approved_account_ids,
        );

        // if not successful, return nft back to original owner
        if !resp {
            NearEvent::log_nft_transfer(receiver_id, previous_owner_id, vec![token_id], None, None);
        }

        resp
    }
}

/// from https://github.com/near/near-sdk-rs/blob/e4abb739ff953b06d718037aa1b8ab768db17348/near-contract-standards/src/non_fungible_token/utils.rs#L29

fn refund_deposit(storage_used: u64, extra_spend: Balance) {
    let required_cost = env::storage_byte_cost() * Balance::from(storage_used);
    let attached_deposit = env::attached_deposit() - extra_spend;

    assert!(
        required_cost <= attached_deposit,
        "Must attach {} yoctoNEAR to cover storage",
        required_cost,
    );

    let refund = attached_deposit - required_cost;
    if refund > 1 {
        Promise::new(env::predecessor_account_id()).transfer(refund);
    }
}

fn to_sec(timestamp: Timestamp) -> TimestampSec {
    (timestamp / 10u64.pow(9)) as u32
}
