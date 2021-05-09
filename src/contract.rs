use cosmwasm_std::{
    entry_point, to_binary, Addr, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Response,
    StdError, StdResult,
};

use crate::error::ContractError;
use crate::msg::{ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{config, config_read, State};

// Note, you can use StdResult in some functions where you do not
// make use of the custom errors
#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    if msg.expires <= _env.block.height {
        return Err(ContractError::Std(StdError::generic_err(
            "Cannot create expired option",
        )));
    }

    let state = State {
        creator: info.sender.clone(),
        owner: info.sender.clone(),
        collateral: info.funds,
        counter_offer: msg.counter_offer,
        expires: msg.expires,
    };
    config(deps.storage).save(&state)?;

    Ok(Response::default())
}

// And declare a custom Error variant for the ones where you will want to make use of it
#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Transfer { recipient } => try_transfer(deps, _env, info, recipient),
        ExecuteMsg::Execute {} => try_execute(deps, _env, info),
        ExecuteMsg::Burn {} => try_burn(deps, _env, info),
    }
}

pub fn try_transfer(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    recipient: Addr,
) -> Result<Response, ContractError> {
    // get state
    let mut state: State = config(deps.storage).load()?;
    // ensure msg.sender is owner
    if info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }

    // set new owner on state
    state.owner = recipient.clone();
    config(deps.storage).save(&state)?;

    let mut res: Response = Response::new();
    res.add_attribute("action", "transfer");
    res.add_attribute("owner", recipient);
    Ok(res)
}

pub fn try_execute(deps: DepsMut, _env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    // get state
    let mut state: State = config(deps.storage).load()?;
    // ensure msg.sender is owner
    if info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }

    // ensure not expired
    if _env.block.height >= state.expires {
        return Err(ContractError::Std(StdError::generic_err("option expired")));
    }

    // ensure sending proper counter_offer
    if info.funds != state.counter_offer {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "must send exact counter_offer: {:?}",
            state.counter_offer
        ))));
    }

    // release counter_offer to creator
    let mut res: Response = Response::new();
    res.add_message(BankMsg::Send {
        to_address: state.creator.as_str().to_string(),
        amount: state.counter_offer,
    });

    // release collateral to sender
    res.add_message(BankMsg::Send {
        to_address: state.owner.as_str().to_string(),
        amount: state.collateral,
    });

    // delete the option
    config(deps.storage).remove();

    res.add_attribute("action", "execute");
    Ok(res)
}

pub fn try_burn(deps: DepsMut, _env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    // get state
    let mut state: State = config(deps.storage).load()?;

    // ensure not expired
    if _env.block.height < state.expires {
        return Err(ContractError::Std(StdError::generic_err("option expired")));
    }

    // ensure sending proper counter_offer
    if !info.funds.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "don't send funds with burn: {:?}",
            state.counter_offer
        ))));
    }

    // release counter_offer to creator
    let mut res: Response = Response::new();
    res.add_message(BankMsg::Send {
        to_address: state.creator.as_str().to_string(),
        amount: state.collateral,
    });

    // delete the option
    config(deps.storage).remove();

    res.add_attribute("action", "burn");
    Ok(res)
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    config_read(deps.storage).load()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, from_binary};

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies(&[]);

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(1000, "earth"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(17, value.count);
    }

    #[test]
    fn increment() {
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let info = mock_info("anyone", &coins(2, "token"));
        let msg = ExecuteMsg::Increment {};
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // should increase counter by 1
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(18, value.count);
    }

    #[test]
    fn reset() {
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let unauth_info = mock_info("anyone", &coins(2, "token"));
        let msg = ExecuteMsg::Reset { count: 5 };
        let res = execute(deps.as_mut(), mock_env(), unauth_info, msg);
        match res {
            Err(ContractError::Unauthorized {}) => {}
            _ => panic!("Must return unauthorized error"),
        }

        // only the original creator can reset the counter
        let auth_info = mock_info("creator", &coins(2, "token"));
        let msg = ExecuteMsg::Reset { count: 5 };
        let _res = execute(deps.as_mut(), mock_env(), auth_info, msg).unwrap();

        // should now be 5
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(5, value.count);
    }
}
