#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/reference/frame-pallets/>
/// <https://docs.substrate.io/reference/frame-pallets/>
/// <https://docs.substrate.io/reference/frame-pallets/>
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

use frame_support::sp_runtime::{
	traits::{AccountIdConversion, StaticLookup, Zero},
	Permill, RuntimeDebug,
};

use frame_support::{
	inherent::Vec,
	pallet_prelude::*,
	traits::{
		Currency, Get, OnUnbalanced,
		ReservableCurrency, UnixTime,
	},
	PalletId,
};

use sp_std::prelude::*;

type AccountIdLookupOf<T> = <<T as frame_system::Config>::Lookup as StaticLookup>::Source;

pub type LoanApy = u64;

pub type NegativeImbalanceOf<T> = <<T as Config>::Currency as Currency<
	<T as frame_system::Config>::AccountId,
>>::NegativeImbalance;

pub type PositiveImbalanceOf<T> = <<T as Config>::Currency as Currency<
	<T as frame_system::Config>::AccountId,
>>::PositiveImbalance;

#[cfg(feature = "runtime-benchmarks")]
pub trait BenchmarkHelper<CollectionId, ItemId> {
	fn to_collection(i: u32) -> CollectionId;
	fn to_nft(i: u32) -> ItemId;
}

#[cfg(feature = "runtime-benchmarks")]
impl<CollectionId: From<u32>, ItemId: From<u32>> BenchmarkHelper<CollectionId, ItemId>
	for NftHelper
{
	fn to_collection(i: u32) -> CollectionId {
		i.into()
	}
	fn to_nft(i: u32) -> ItemId {
		i.into()
	}
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_system::pallet_prelude::*;
	use scale_info::TypeInfo;

	type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	pub type ProposalIndex = u32;
	pub type LoanIndex = u32;

	type BalanceOf1<T> = <<T as pallet_contracts::Config>::Currency as Currency<
		<T as frame_system::Config>::AccountId,
	>>::Balance;

	/// A loan proposal
	#[cfg_attr(feature = "std", derive(serde::Serialize, serde::Deserialize))]
	#[derive(Encode, Decode, Clone, PartialEq, Eq, MaxEncodedLen, RuntimeDebug, TypeInfo)]
	pub struct Proposal<AccountId, Balance> {
		proposer: AccountId,
		amount: Balance,
		beneficiary: AccountId,
		bond: Balance,
	}

	/// loan info
	#[cfg_attr(feature = "std", derive(serde::Serialize, serde::Deserialize))]
	#[derive(Encode, Decode, Clone, PartialEq, Eq, MaxEncodedLen, RuntimeDebug, TypeInfo)]
	pub struct LoanInfo<AccountId, Balance, CollectionId, ItemId> {
		borrower: AccountId,
		amount: Balance,
		collection_id: CollectionId,
		item_id: ItemId,
		loan_apy: LoanApy,
		last_timestamp: u64,
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config:
		frame_system::Config + pallet_uniques::Config + pallet_contracts::Config
	{
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The currency type.
		type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;

		/// Origin from which rejections must come.
		type RejectOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Origin from which approves must come.
		type ApproveOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Fraction of a proposal's value that should be bonded in order to place the proposal.
		/// An accepted proposal gets these back. A rejected proposal does not.
		#[pallet::constant]
		type ProposalBond: Get<Permill>;

		/// Minimum amount of funds that should be placed in a deposit for making a proposal.
		#[pallet::constant]
		type ProposalBondMinimum: Get<BalanceOf<Self>>;

		/// Maximum amount of funds that should be placed in a deposit for making a proposal.
		#[pallet::constant]
		type ProposalBondMaximum: Get<Option<BalanceOf<Self>>>;

		/// The treasury's pallet id, used for deriving its sovereign account ID.
		#[pallet::constant]
		type PalletId: Get<PalletId>;

		/// Handler for the unbalanced decrease when slashing for a rejected proposal or bounty.
		type OnSlash: OnUnbalanced<NegativeImbalanceOf<Self>>;

		/// The maximum amount of loans that can run at the same time.
		#[pallet::constant]
		type MaxOngoingLoans: Get<u32>;

		/// lose coupling of pallet timestamp
		type TimeProvider: UnixTime;
	}

	/// Number of proposals that have been made.
	#[pallet::storage]
	#[pallet::getter(fn loan_count)]
	pub(super) type LoanCount<T> = StorageValue<_, ProposalIndex, ValueQuery>;

	/// Number of proposals that have been made.
	#[pallet::storage]
	#[pallet::getter(fn proposal_count)]
	pub(super) type ProposalCount<T> = StorageValue<_, ProposalIndex, ValueQuery>;

	/// All currently ongoing loans
	#[pallet::storage]
	#[pallet::getter(fn ongoing_loans)]
	pub(super) type OngoingLoans<T: Config> =
		StorageValue<_, BoundedVec<ProposalIndex, T::MaxOngoingLoans>, ValueQuery>;

	/// Proposals that have been made.
	#[pallet::storage]
	#[pallet::getter(fn proposals)]
	pub(super) type Proposals<T: Config> = StorageMap<
		_,
		Twox64Concat,
		ProposalIndex,
		Proposal<T::AccountId, BalanceOf<T>>,
		OptionQuery,
	>;

	/// Mapping of ongoing loans
	#[pallet::storage]
	#[pallet::getter(fn loans)]
	pub(super) type Loans<T: Config> = StorageMap<
		_,
		Twox64Concat,
		LoanIndex,
		LoanInfo<T::AccountId, BalanceOf<T>, T::CollectionId, T::ItemId>,
		OptionQuery,
	>;

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		#[serde(skip)]
		_config: sp_std::marker::PhantomData<T>,
	}

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
		fn build(&self) {
			// Create Treasury account
			let account_id = <Pallet<T>>::account_id();
			let min = <T as pallet::Config>::Currency::minimum_balance();
			if <T as pallet::Config>::Currency::free_balance(&account_id) < min {
				let _ = <T as pallet::Config>::Currency::make_free_balance_be(&account_id, min);
			}
		}
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Proposer's balance is too low
		InsufficientProposersBalance,
		/// Loan pool's balance is too low
		InsufficientLoanPoolBalance,
		/// No proposal index
		InvalidIndex,
		/// There are too many loan proposals
		InsufficientPermission,
		/// Max amount of ongoing loan reached
		TooManyLoans,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// New Proposal
		Proposed { proposal_index: ProposalIndex },
		/// Proposal has been approved
		Approved { proposal_index: ProposalIndex },
		/// Proposal has been rejected
		Rejected { proposal_index: ProposalIndex },
	}

	// Work in progress, to be included in the future
	/*  	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		/// ## Complexity
		/// - `O(A)` where `A` is the number of approvals
		fn on_initialize(n: frame_system::pallet_prelude::BlockNumberFor<T>) -> Weight {
			Self::charge_apy();
			let used_weight = T::DbWeight::get().writes(1);
			used_weight

		}
	}  */

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Apply for a loan. A deposit amount is reserved
		/// and slashed if the proposal is rejected. It is returned once the proposal is awarded.
		#[pallet::call_index(0)]
		#[pallet::weight(0)]
		pub fn propose(
			origin: OriginFor<T>,
			amount: BalanceOf<T>,
			beneficiary: AccountIdLookupOf<T>,
		) -> DispatchResult {
			let origin = ensure_signed(origin)?;
			let beneficiary = T::Lookup::lookup(beneficiary)?;
			let proposal_index = Self::proposal_count() + 1;
			let bond = Self::calculate_bond(amount);
			<T as pallet::Config>::Currency::reserve(&origin, bond)
				.map_err(|_| Error::<T>::InsufficientProposersBalance)?;

			let proposal = Proposal {
				proposer: origin.clone(),
				amount,
				beneficiary: beneficiary.clone(),
				bond,
			};
			Proposals::<T>::insert(proposal_index, proposal);
			ProposalCount::<T>::put(proposal_index);

			Self::deposit_event(Event::Proposed { proposal_index });
			Ok(())
		}

		/// Reject a proposed spend. The original deposit will be slashed.
		///
		/// May only be called from `T::RejectOrigin`.
		#[pallet::call_index(1)]
		#[pallet::weight(0)]
		pub fn reject_proposal(
			origin: OriginFor<T>,
			proposal_index: ProposalIndex,
		) -> DispatchResult {
			T::RejectOrigin::ensure_origin(origin)?;
			let proposal = <Proposals<T>>::take(&proposal_index).ok_or(Error::<T>::InvalidIndex)?;
			let value = proposal.bond;
			let imbalance =
				<T as pallet::Config>::Currency::slash_reserved(&proposal.proposer, value).0;
			T::OnSlash::on_unbalanced(imbalance);

			Proposals::<T>::remove(proposal_index);

			Self::deposit_event(Event::<T>::Rejected { proposal_index });
			Ok(())
		}

		/// Approve a proposed spend. The original deposit will be released.
		/// It will call the create_loan function in the contract and deposit the loan amount in
		/// the contract.
		///
		/// May only be called from `T::ApproveOrigin`.

		#[pallet::call_index(2)]
		#[pallet::weight(0)]
		pub fn approve_proposal(
			origin: OriginFor<T>,
			proposal_index: ProposalIndex,
			collection_id: T::CollectionId,
			collateral_price: BalanceOf<T>,
			value_funds: BalanceOf1<T>,
			item_id: T::ItemId,
			loan_apy: LoanApy,
			dest: T::AccountId,
			admin: T::AccountId,
			storage_deposit_limit: Option<BalanceOf1<T>>,
			gas_limit: Weight,
		) -> DispatchResult {
			let _signer = ensure_signed(origin.clone())?;
			//T::ApproveOrigin::ensure_origin(origin.clone())?;
			let proposal = <Proposals<T>>::take(&proposal_index).ok_or(Error::<T>::InvalidIndex)?;
			let err_amount =
				<T as pallet::Config>::Currency::unreserve(&proposal.proposer, proposal.bond);
			debug_assert!(err_amount.is_zero());
			let user = proposal.beneficiary;
			let value = proposal.amount;
			let timestamp = T::TimeProvider::now().as_secs();

			let loan_info = LoanInfo {
				borrower: user.clone(),
				amount: value,
				collection_id,
				item_id,
				loan_apy,
				last_timestamp: timestamp,
			};

			let loan_index = Self::loan_count() + 1;

			Loans::<T>::insert(loan_index, loan_info);
			OngoingLoans::<T>::try_append(loan_index).map_err(|_| Error::<T>::TooManyLoans)?;
			/// calls the create collection function from the uniques pallet, and set the admin as
			/// the admin of the collection
			pallet_uniques::Pallet::<T>::do_create_collection(
				collection_id,
				admin.clone(),
				admin.clone(),
				T::CollectionDeposit::get(),
				false,
				pallet_uniques::Event::Created {
					creator: admin.clone(),
					owner: admin.clone(),
					collection: collection_id,
				},
			)?;
			/// calls the mint collection function from the uniques pallet, mints a nft and puts
			/// the loan contract as the owner
			pallet_uniques::Pallet::<T>::do_mint(collection_id, item_id, dest.clone(), |_| Ok(()))?;

			let palled_id = Self::account_id();
			let value = proposal.amount;
			let mut arg1_enc: Vec<u8> = admin.encode();
			let mut arg2_enc: Vec<u8> = user.encode();
			let mut arg3_enc: Vec<u8> = collection_id.clone().encode();
			let mut arg4_enc: Vec<u8> = item_id.clone().encode();
			let mut arg5_enc: Vec<u8> = collateral_price.encode();
			let mut arg6_enc: Vec<u8> = value.clone().encode();
			let mut data = Vec::new();
			let mut selector: Vec<u8> = [0x0E, 0xA6, 0xbd, 0x42].into();
			data.append(&mut selector);
			data.append(&mut arg1_enc);
			data.append(&mut arg2_enc);
			data.append(&mut arg3_enc);
			data.append(&mut arg4_enc);
			data.append(&mut arg5_enc);
			data.append(&mut arg6_enc);

			/// Calls the creat loan function of the loan smart contract
			pallet_contracts::Pallet::<T>::bare_call(
				palled_id.clone(),
				dest.clone(),
				value_funds,
				gas_limit,
				storage_deposit_limit,
				data,
				false,
				pallet_contracts::Determinism::Deterministic,
			)
			.result?;
			Proposals::<T>::remove(proposal_index);
			LoanCount::<T>::put(loan_index);
			Self::deposit_event(Event::<T>::Approved { proposal_index });
			Ok(())
		}

		/// Delete the loan after the loan is paid back. The collateral nft will be deleted.
		///
		/// May only be called from the loan contract.

		#[pallet::call_index(3)]
		#[pallet::weight(0)]
		pub fn delete_loan(origin: OriginFor<T>, loan_id: LoanIndex) -> DispatchResult {
			let _signer = ensure_signed(origin.clone())?;
			let loan = <Loans<T>>::take(&loan_id).ok_or(Error::<T>::InvalidIndex)?;

			let collection_id = loan.collection_id;
			let item_id = loan.item_id;

			pallet_uniques::Pallet::<T>::do_burn(collection_id, item_id, |_, _| Ok(()))?;

			let mut loans = Self::ongoing_loans();
			let index = loans.iter().position(|x| *x == loan_id).unwrap();
			loans.remove(index);

			OngoingLoans::<T>::put(loans);
			Loans::<T>::remove(loan_id);
			Ok(())
		}
	}

	//** Our helper functions.**//

	impl<T: Config> Pallet<T> {
		pub fn account_id() -> T::AccountId {
			T::PalletId::get().into_account_truncating()
		}

		fn calculate_bond(value: BalanceOf<T>) -> BalanceOf<T> {
			let mut r = T::ProposalBondMinimum::get().max(T::ProposalBond::get() * value);
			if let Some(m) = T::ProposalBondMaximum::get() {
				r = r.min(m);
			}
			r
		}

		// Work in progress, to be implmented in the future
		pub fn charge_apy() -> DispatchResult {
			let ongoing_loans = Self::ongoing_loans();
			let mut index = 0;
			for _i in ongoing_loans.clone() {
				let loan_index = ongoing_loans[index];
				let mut loan = <Loans<T>>::take(&loan_index).ok_or(Error::<T>::InvalidIndex)?;
				let current_timestamp = T::TimeProvider::now().as_secs();
				let time_difference = current_timestamp - loan.last_timestamp;
				let loan_amount = Self::balance_to_u64(loan.amount).unwrap();
				let interests =
					loan_amount * time_difference * loan.loan_apy / 365 / 60 / 60 / 24 / 100;
				let interest_balance = Self::u64_to_balance_option(interests).unwrap();
				loan.amount = loan.amount + interest_balance;
				loan.last_timestamp = current_timestamp;
				index += 1;
			}
			Ok(())
		}

		pub fn balance_to_u64(input: BalanceOf<T>) -> Option<u64> {
			TryInto::<u64>::try_into(input).ok()
		}

		pub fn u64_to_balance_option(input: u64) -> Option<BalanceOf<T>> {
			input.try_into().ok()
		}
	}
}
