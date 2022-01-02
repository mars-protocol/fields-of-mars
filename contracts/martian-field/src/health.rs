use cosmwasm_std::{Decimal, Env, QuerierWrapper, StdResult, Uint128};

use fields_of_mars::martian_field::{Config, Health, Position, State};

/// This module is purely a workaround that lets us ignore lints for all the code the `construct_uint!`
/// macro generates
#[allow(clippy::all)]
mod uints {
    uint::construct_uint! {
        pub struct U256(4);
    }
}

/// Used internally - we don't want to leak this type since we might change the implementation in the future
use uints::U256;

/// Compute the health of a user's position
pub fn compute_health(
    querier: &QuerierWrapper,
    env: &Env,
    config: &Config,
    state: &State,
    position: &Position,
) -> StdResult<Health> {
    let total_bonded_amount = config.astro_generator.query_bonded_amount(
        querier,
        &env.contract.address,
        &config.primary_pair.liquidity_token,
    )?;

    let total_debt_amount = config.red_bank.query_user_debt(
        querier,
        &env.contract.address,
        &config.secondary_asset_info,
    )?;

    let (primary_depth, secondary_depth, total_shares) = config.primary_pair.query_pool(
        querier,
        &config.primary_asset_info,
        &config.secondary_asset_info,
    )?;

    let primary_price = config.oracle.query_price(querier, &config.primary_asset_info)?;
    let secondary_price = config.oracle.query_price(querier, &config.secondary_asset_info)?;

    // RE the calculation of the value of liquidity token, see:
    // https://blog.alphafinance.io/fair-lp-token-pricing/
    // this formulation avoids a potential sandwich attack that distorts asset prices by a flashloan
    //
    // NOTE: we need to use U256 here, because Uint128 * Uint128 may overflow the 128-bit limit
    let primary_value = U256::from(u128::from(primary_depth * primary_price));
    let secondary_value = U256::from(u128::from(secondary_depth * secondary_price));
    let pool_value = U256::from(2) * (primary_value * secondary_value).integer_sqrt();

    let pool_value_u128 = Uint128::new(pool_value.as_u128());
    let total_bonded_value = pool_value_u128.multiply_ratio(total_bonded_amount, total_shares);

    // compute the value of the user's bonded liquidity tokens
    let bond_value = if state.total_bond_units.is_zero() {
        Uint128::zero()
    } else {
        total_bonded_value.multiply_ratio(position.bond_units, state.total_bond_units)
    };

    // compute the value of the user's debt
    let debt_value = if state.total_debt_units.is_zero() {
        Uint128::zero()
    } else {
        total_debt_amount.multiply_ratio(position.debt_units, state.total_debt_units)
    };

    // compute LTV
    // if the position is closed (i.e. the user doesn't have any liquidity token bonded) then LTV is
    // undefined. return None is this case
    // otherwise, LTV is defined, return Some(ltv) in this case
    let ltv = if bond_value.is_zero() {
        None
    } else {
        Some(Decimal::from_ratio(debt_value, bond_value))
    };

    Ok(Health {
        bond_value,
        debt_value,
        ltv,
    })
}