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

const DATA_IMAGE_SVG_PARAS_ICON: &str = "data:image/jpeg;base64,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AxFLteJ7mtiHqL3vNBn8rzKRt3gwRg9rQppmxx9AAmy+ERIkmJndK7qtdEABhzIRogKKamh0vNSw6gh7uAHGgN3hT1oR93WtIJoOCYOaBZpiSMYDEGIvGG3EbArK/wAFw8NhM+EIUAh8C6WG44oMGwc2hXBMY3xgpBFQoz4wo5DDDqiEAxwPAJjccUZxZhqaiTa+ESaBDZHPxQAAhziMHrGKZoX3UyQcA7U0oGBUmWsxcbhS0QThk0oTQUbsXewggBJwCbCehVNRvXnf+nKi2L1bHIC0UJa4iaOSLAEUT/3KkHuRQlgAFsCBMDRAm7ogNh8+RCk77JVASIBjwX4GQGNgo0l0UVX2k+eAaDDIY2EVXGgqSgOpTD4CmfqTv1ISXyFhLco+WNiDABDU2BEsE254yO18erZCgMm+XxZ0fvk7y+LOh90MmnD3sB8nI4P5GwX/AHTJzQwAWObSOTStdrHwkZN0VFu8r5NZIxJztYASQBNaXGyYTAkr28Dax1UybNOIui2Clc+Mlddwoin8VATraDBEwlpJ1UCo8GL9ZGJwMYllBjJoXVJSQr2hdRkjelb4AmU5ENJfcH4UOFsJyCZiAmSmnefiAT5ZqeM1x98E2burtz4lDJyUeiID/ablOEbhQPquunPcVwOwTWMmTWta1rJkyZMiIHABcFOXTJ+OeMmTBMEwTBMmTJgmTJgmCZMmTJgm/hn/AP/aAAwDAQACAAMAAAAQ8888s8888888888888888888888888888888888888888888888888888888888880808888888888888888888888scsMsMc88888888888888888888888888888888888888888888888888888888888888888888888888w040888ww0w8888888888888w088888888888888888888888888888888888888888888888888888888888888888888888888888888888c8c88888888888888888888888888888888sc8Ms888888888888888888888888888J888888888888888888888488888888888888886u08888888888888888888888888888888888888j/APfPPPPPPPPPPPNPPPPPPPPPPPPPPPPPPPPP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";

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
                name: "Fella Collectibles".to_string(),
                symbol: "FELLA".to_string(),
                icon: Some(DATA_IMAGE_SVG_PARAS_ICON.to_string()),
                base_uri: Some("https://bafybeidoz6g3eizzloxfst76mulxvkj7mabh6wnoayrgalhpk4ddjjiggy.ipfs.nftstorage.link/".to_string()),
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
        
        if contain {
            let balance = self.account_id_og.get(account_id).unwrap().clone();
            if balance > 0 {
                true
            }else {
                false
            }
        }else{
            false
        }
    }

    pub fn get_og_balance(&self, account_id: &AccountId) -> u32 {
        let contain = self.account_id_og.contains_key(account_id);
        
        if contain {
            self.account_id_og.get(account_id).unwrap().clone()
        }else{
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
    pub fn nft_buy(
        &mut self,
        token_series_id: TokenSeriesId,
        receiver_id: ValidAccountId,
    ) -> TokenId {
        let initial_storage_usage = env::storage_usage();

        let token_series = self
            .token_series_by_id
            .get(&token_series_id)
            .expect("Paras: Token series not exist");
        let price: u128 = token_series.price.expect("Paras: not for sale");
        let attached_deposit = env::attached_deposit();
        assert!(
            attached_deposit >= price,
            "Paras: attached deposit is less than price : {}",
            price
        );
        let token_id: TokenId =
            self._nft_mint_series(token_series_id.clone(), receiver_id.to_string());

        let for_treasury = price as u128
            * self.calculate_market_data_transaction_fee(&token_series_id)
            / 10_000u128;
        let price_deducted = price - for_treasury;
        Promise::new(token_series.creator_id).transfer(price_deducted);

        if for_treasury != 0 {
            Promise::new(self.treasury_id.clone()).transfer(for_treasury);
        }

        refund_deposit(env::storage_usage() - initial_storage_usage, price);

        NearEvent::log_nft_mint(
            receiver_id.to_string(),
            vec![token_id.clone()],
            Some(json!({"price": price.to_string()}).to_string()),
        );

        token_id
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
        let token_series_id = (self.raffle.draw() + 1).to_string(); //random token series id from 1 to max size
                                                                    // log(token_series_id.as_bytes());
        self.token_series_by_id
            .get(&token_series_id)
            .expect("Paras: Token series not exist");
        // let token_series = self.token_series_by_id.get(&token_series_id).expect("Paras: Token series not exist");
        // assert_eq!(env::predecessor_account_id(), token_series.creator_id, "Paras: not creator");
        let token_id: TokenId = self._nft_mint_series(token_series_id, receiver_id.to_string());

        ext_whitelist_contract::incress_balance_whitelist(
            caller.clone(),
            &self.whitelist_contract_id.clone(),
            NO_DEPOSIT,
            GAS_FOR_RESOLVE_TRANSFER,
        );

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

        token_metadata.title = Some(format!(
            "{}{}{}",
            series_metadata.title.unwrap(),
            TITLE_DELIMETER,
            token_id_iter.next().unwrap()
        ));

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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::testing_env;
    use near_sdk::MockedBlockchain;

    const STORAGE_FOR_CREATE_SERIES: Balance = 8540000000000000000000;
    const STORAGE_FOR_MINT: Balance = 11280000000000000000000;

    fn get_context(predecessor_account_id: ValidAccountId) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(accounts(0))
            .signer_account_id(predecessor_account_id.clone())
            .predecessor_account_id(predecessor_account_id);
        builder
    }

    fn setup_contract() -> (VMContextBuilder, Contract) {
        let mut context = VMContextBuilder::new();
        testing_env!(context.predecessor_account_id(accounts(0)).build());
        let contract = Contract::new_default_meta(accounts(0), accounts(4), "".to_string(), 0);
        (context, contract)
    }

    #[test]
    fn test_new() {
        let mut context = get_context(accounts(1));
        testing_env!(context.build());
        let contract = Contract::new(
            accounts(1),
            accounts(4),
            0,
            NFTContractMetadata {
                spec: NFT_METADATA_SPEC.to_string(),
                name: "Triple Triad".to_string(),
                symbol: "TRIAD".to_string(),
                icon: Some(DATA_IMAGE_SVG_PARAS_ICON.to_string()),
                base_uri: Some("https://ipfs.fleek.co/ipfs/".to_string()),
                reference: None,
                reference_hash: None,
            },
            "".to_string(),
            500,
        );
        testing_env!(context.is_view(true).build());
        assert_eq!(contract.get_owner(), accounts(1).to_string());
        assert_eq!(
            contract.nft_metadata().base_uri.unwrap(),
            "https://ipfs.fleek.co/ipfs/".to_string()
        );
        assert_eq!(
            contract.nft_metadata().icon.unwrap(),
            DATA_IMAGE_SVG_PARAS_ICON.to_string()
        );
    }

    fn create_series(
        contract: &mut Contract,
        royalty: &HashMap<AccountId, u32>,
        price: Option<U128>,
        copies: Option<u64>,
    ) {
        contract.nft_create_series(
            None,
            TokenMetadata {
                title: Some("Tsundere land".to_string()),
                description: None,
                media: Some(
                    "bafybeidzcan4nzcz7sczs4yzyxly4galgygnbjewipj6haco4kffoqpkiy".to_string(),
                ),
                media_hash: None,
                copies: copies,
                issued_at: None,
                expires_at: None,
                starts_at: None,
                updated_at: None,
                extra: None,
                reference: Some(
                    "bafybeicg4ss7qh5odijfn2eogizuxkrdh3zlv4eftcmgnljwu7dm64uwji".to_string(),
                ),
                reference_hash: None,
            },
            price,
            Some(royalty.clone()),
        );
    }

    #[test]
    fn test_create_series() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);
        create_series(
            &mut contract,
            &royalty,
            Some(U128::from(1 * 10u128.pow(24))),
            None,
        );

        let nft_series_return = contract.nft_get_series_single("1".to_string());
        assert_eq!(nft_series_return.creator_id, accounts(1).to_string());

        assert_eq!(nft_series_return.token_series_id, "1",);

        assert_eq!(nft_series_return.royalty, royalty,);

        assert!(nft_series_return.metadata.copies.is_none());

        assert_eq!(
            nft_series_return.metadata.title.unwrap(),
            "Tsundere land".to_string()
        );

        assert_eq!(
            nft_series_return.metadata.reference.unwrap(),
            "bafybeicg4ss7qh5odijfn2eogizuxkrdh3zlv4eftcmgnljwu7dm64uwji".to_string()
        );
    }

    #[test]
    fn test_buy() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(
            &mut contract,
            &royalty,
            Some(U128::from(1 * 10u128.pow(24))),
            None,
        );

        testing_env!(context
            .predecessor_account_id(accounts(2))
            .attached_deposit(1 * 10u128.pow(24) + STORAGE_FOR_MINT)
            .build());

        let token_id = contract.nft_buy("1".to_string(), accounts(2));

        let token_from_nft_token = contract.nft_token(token_id);
        assert_eq!(
            token_from_nft_token.unwrap().owner_id,
            accounts(2).to_string()
        )
    }

    #[test]
    fn test_mint() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(&mut contract, &royalty, None, None);

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_MINT)
            .build());

        let token_id = contract.nft_mint_creator("1".to_string(), accounts(2));

        let token_from_nft_token = contract.nft_token(token_id);
        assert_eq!(
            token_from_nft_token.unwrap().owner_id,
            accounts(2).to_string()
        )
    }

    #[test]
    #[should_panic(expected = "Paras: Token series is not mintable")]
    fn test_invalid_mint_non_mintable() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(&mut contract, &royalty, None, None);

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(1)
            .build());
        contract.nft_set_series_non_mintable("1".to_string());

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_MINT)
            .build());

        contract.nft_mint_creator("1".to_string(), accounts(2));
    }

    #[test]
    #[should_panic(expected = "Paras: Token series is not mintable")]
    fn test_invalid_mint_above_copies() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(&mut contract, &royalty, None, Some(1));

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_MINT)
            .build());

        contract.nft_mint_creator("1".to_string(), accounts(2));
        contract.nft_mint_creator("1".to_string(), accounts(2));
    }

    #[test]
    fn test_decrease_copies() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(&mut contract, &royalty, None, Some(5));

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_MINT)
            .build());

        contract.nft_mint_creator("1".to_string(), accounts(2));
        contract.nft_mint_creator("1".to_string(), accounts(2));

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(1)
            .build());

        contract.nft_decrease_series_copies("1".to_string(), U64::from(3));
    }

    #[test]
    #[should_panic(expected = "Paras: cannot decrease supply, already minted : 2")]
    fn test_invalid_decrease_copies() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(&mut contract, &royalty, None, Some(5));

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_MINT)
            .build());

        contract.nft_mint_creator("1".to_string(), accounts(2));
        contract.nft_mint_creator("1".to_string(), accounts(2));

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(1)
            .build());

        contract.nft_decrease_series_copies("1".to_string(), U64::from(4));
    }

    #[test]
    #[should_panic(expected = "Paras: not for sale")]
    fn test_invalid_buy_price_null() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(
            &mut contract,
            &royalty,
            Some(U128::from(1 * 10u128.pow(24))),
            None,
        );

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(1)
            .build());

        contract.nft_set_series_price("1".to_string(), None);

        testing_env!(context
            .predecessor_account_id(accounts(2))
            .attached_deposit(1 * 10u128.pow(24) + STORAGE_FOR_MINT)
            .build());

        let token_id = contract.nft_buy("1".to_string(), accounts(2));

        let token_from_nft_token = contract.nft_token(token_id);
        assert_eq!(
            token_from_nft_token.unwrap().owner_id,
            accounts(2).to_string()
        )
    }

    #[test]
    #[should_panic(expected = "Paras: price higher than 1000000000000000000000000000000000")]
    fn test_invalid_price_shouldnt_be_higher_than_max_price() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(
            &mut contract,
            &royalty,
            Some(U128::from(1_000_000_000 * 10u128.pow(24))),
            None,
        );

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(1)
            .build());
    }

    #[test]
    fn test_nft_burn() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(&mut contract, &royalty, None, None);

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_MINT)
            .build());

        let token_id = contract.nft_mint_creator("1".to_string(), accounts(2));

        testing_env!(context
            .predecessor_account_id(accounts(2))
            .attached_deposit(1)
            .build());

        contract.nft_burn(token_id.clone());
        let token = contract.nft_token(token_id);
        assert!(token.is_none());
    }

    #[test]
    fn test_nft_transfer() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(&mut contract, &royalty, None, None);

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_MINT)
            .build());

        let token_id = contract.nft_mint_creator("1".to_string(), accounts(2));

        testing_env!(context
            .predecessor_account_id(accounts(2))
            .attached_deposit(1)
            .build());

        contract.nft_transfer(accounts(3), token_id.clone(), None, None);

        let token = contract.nft_token(token_id).unwrap();
        assert_eq!(token.owner_id, accounts(3).to_string())
    }

    #[test]
    fn test_nft_transfer_unsafe() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(&mut contract, &royalty, None, None);

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_MINT)
            .build());

        let token_id = contract.nft_mint_creator("1".to_string(), accounts(2));

        testing_env!(context.predecessor_account_id(accounts(2)).build());

        contract.nft_transfer_unsafe(accounts(3), token_id.clone(), None, None);

        let token = contract.nft_token(token_id).unwrap();
        assert_eq!(token.owner_id, accounts(3).to_string())
    }

    #[test]
    fn test_nft_transfer_payout() {
        let (mut context, mut contract) = setup_contract();
        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        create_series(&mut contract, &royalty, None, None);

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .attached_deposit(STORAGE_FOR_MINT)
            .build());

        let token_id = contract.nft_mint_creator("1".to_string(), accounts(2));

        testing_env!(context
            .predecessor_account_id(accounts(2))
            .attached_deposit(1)
            .build());

        let payout = contract.nft_transfer_payout(
            accounts(3),
            token_id.clone(),
            Some(0),
            Some(U128::from(1 * 10u128.pow(24))),
            Some(10),
        );

        let mut payout_calc: HashMap<AccountId, U128> = HashMap::new();
        payout_calc.insert(
            accounts(1).to_string(),
            U128::from((1000 * (1 * 10u128.pow(24))) / 10_000),
        );
        payout_calc.insert(
            accounts(2).to_string(),
            U128::from((9000 * (1 * 10u128.pow(24))) / 10_000),
        );

        assert_eq!(payout.unwrap().payout, payout_calc);

        let token = contract.nft_token(token_id).unwrap();
        assert_eq!(token.owner_id, accounts(3).to_string())
    }

    #[test]
    fn test_change_transaction_fee_immediately() {
        let (mut context, mut contract) = setup_contract();

        testing_env!(context
            .predecessor_account_id(accounts(0))
            .attached_deposit(1)
            .build());

        contract.set_transaction_fee(100, None);

        assert_eq!(contract.get_transaction_fee().current_fee, 100);
    }

    #[test]
    fn test_change_transaction_fee_with_time() {
        let (mut context, mut contract) = setup_contract();

        testing_env!(context
            .predecessor_account_id(accounts(0))
            .attached_deposit(1)
            .build());

        assert_eq!(contract.get_transaction_fee().current_fee, 500);
        assert_eq!(contract.get_transaction_fee().next_fee, None);
        assert_eq!(contract.get_transaction_fee().start_time, None);

        let next_fee: u16 = 100;
        let start_time: Timestamp = 1618109122863866400;
        let start_time_sec: TimestampSec = to_sec(start_time);
        contract.set_transaction_fee(next_fee, Some(start_time_sec));

        assert_eq!(contract.get_transaction_fee().current_fee, 500);
        assert_eq!(contract.get_transaction_fee().next_fee, Some(next_fee));
        assert_eq!(
            contract.get_transaction_fee().start_time,
            Some(start_time_sec)
        );

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .block_timestamp(start_time + 1)
            .build());

        contract.calculate_current_transaction_fee();
        assert_eq!(contract.get_transaction_fee().current_fee, next_fee);
        assert_eq!(contract.get_transaction_fee().next_fee, None);
        assert_eq!(contract.get_transaction_fee().start_time, None);
    }

    #[test]
    fn test_transaction_fee_locked() {
        let (mut context, mut contract) = setup_contract();

        testing_env!(context
            .predecessor_account_id(accounts(0))
            .attached_deposit(1)
            .build());

        assert_eq!(contract.get_transaction_fee().current_fee, 500);
        assert_eq!(contract.get_transaction_fee().next_fee, None);
        assert_eq!(contract.get_transaction_fee().start_time, None);

        let next_fee: u16 = 100;
        let start_time: Timestamp = 1618109122863866400;
        let start_time_sec: TimestampSec = to_sec(start_time);
        contract.set_transaction_fee(next_fee, Some(start_time_sec));

        let mut royalty: HashMap<AccountId, u32> = HashMap::new();
        royalty.insert(accounts(1).to_string(), 1000);

        testing_env!(context
            .predecessor_account_id(accounts(0))
            .attached_deposit(STORAGE_FOR_CREATE_SERIES)
            .build());

        create_series(
            &mut contract,
            &royalty,
            Some(U128::from(1 * 10u128.pow(24))),
            None,
        );

        testing_env!(context
            .predecessor_account_id(accounts(0))
            .attached_deposit(1)
            .build());

        contract.nft_set_series_price("1".to_string(), None);

        assert_eq!(contract.get_transaction_fee().current_fee, 500);
        assert_eq!(contract.get_transaction_fee().next_fee, Some(next_fee));
        assert_eq!(
            contract.get_transaction_fee().start_time,
            Some(start_time_sec)
        );

        testing_env!(context
            .predecessor_account_id(accounts(1))
            .block_timestamp(start_time + 1)
            .attached_deposit(1)
            .build());

        contract.calculate_current_transaction_fee();
        assert_eq!(contract.get_transaction_fee().current_fee, next_fee);
        assert_eq!(contract.get_transaction_fee().next_fee, None);
        assert_eq!(contract.get_transaction_fee().start_time, None);

        let series = contract.nft_get_series_single("1".to_string());
        let series_transaction_fee: u128 = series.transaction_fee.unwrap().into();
        assert_eq!(series_transaction_fee, 500);
    }
}
