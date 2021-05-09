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
    let state: State = config(deps.storage).load()?;
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
    let state: State = config(deps.storage).load()?;

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
    use cosmwasm_std::{coins, Attribute, CosmosMsg};

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies(&[]);

        let msg = InstantiateMsg {
            counter_offer: coins(40, "ETH"),
            expires: 100_000,
        };
        let _env = mock_env();
        let info = mock_info("creator", &coins(1, "BTC"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), _env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let res: State = query_config(deps.as_ref()).unwrap();
        assert_eq!(100_000, res.expires);
        assert_eq!("creator", res.owner.as_str());
        assert_eq!("creator", res.creator.as_str());
        assert_eq!(coins(1, "BTC"), res.collateral);
        assert_eq!(coins(40, "ETH"), res.counter_offer);
    }

    #[test]
    fn transfer() {
        let mut deps = mock_dependencies(&coins(2, "token"));
        // // we can just call .unwrap() to assert this was a success
        let msg = InstantiateMsg {
            counter_offer: coins(40, "ETH"),
            expires: 100_000,
        };
        let _env = mock_env();
        let info = mock_info("creator", &coins(1, "BTC"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), _env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // random cannot transfer
        let _env = mock_env();
        let info = mock_info("anyone", &[]);
        let err = try_transfer(deps.as_mut(), _env, info, Addr::unchecked("anyone")).unwrap_err();
        match err {
            ContractError::Unauthorized { .. } => {}
            e => panic!("unexpected: {}", e),
        }

        let _env = mock_env();
        // owner can transfer
        let info = mock_info("creator", &[]);
        let res = try_transfer(deps.as_mut(), _env, info, Addr::unchecked("someone")).unwrap();
        assert_eq!(res.attributes.len(), 2);
        assert_eq!(
            res.attributes[0],
            Attribute {
                key: "action".to_string(),
                value: "transfer".to_string(),
            }
        );
        let res: State = query_config(deps.as_ref()).unwrap();
        assert_eq!("someone", res.owner.as_str());
        assert_eq!("creator", res.creator.as_str());
    }

    #[test]
    fn execute() {
        let mut deps = mock_dependencies(&coins(2, "token"));
        // // we can just call .unwrap() to assert this was a success
        let counter_offer = coins(40, "ETH");
        let collateral = coins(1, "BTC");
        let msg = InstantiateMsg {
            counter_offer: counter_offer.clone(),
            expires: 100_000,
        };
        let _env = mock_env();
        let info = mock_info("creator", &coins(1, "BTC"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), _env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // set new owner
        let _env = mock_env();
        let info = mock_info("creator", &[]);
        let _ = try_transfer(deps.as_mut(), _env, info, Addr::unchecked("owner")).unwrap();

        // random cannot execute
        let info = mock_info("anyone", &[]);
        let err = try_execute(deps.as_mut(), mock_env(), info).unwrap_err();
        match err {
            ContractError::Unauthorized { .. } => {}
            e => panic!("unexpected: {}", e),
        }

        // expired cannot execute
        let mut _env = mock_env();
        _env.block.height = 200_000;
        let info = mock_info("owner", &counter_offer);
        let err = try_execute(deps.as_mut(), _env, info).unwrap_err();

        match err {
            ContractError::Std(from) => match from {
                StdError::GenericErr { msg, .. } => assert_eq!("option expired", msg.as_str()),
                e => panic!("unexpected: {}", e),
            },
            e => panic!("unexpected: {}", e),
        }

        // bad counter_offer cannot execute
        let info = mock_info("owner", &coins(39, "ETH"));
        let err = try_execute(deps.as_mut(), mock_env(), info).unwrap_err();

        match err {
            ContractError::Std(from) => match from {
                StdError::GenericErr { msg, .. } => {
                    assert_eq!("must send exact counter_offer: [Coin { denom: \"ETH\", amount: Uint128(40) }]", msg.as_str())
                }
                e => panic!("unexpected: {}", e),
            },
            e => panic!("unexpected: {}", e),
        }

        // proper execution
        let mut _env = mock_env();
        let info = mock_info("owner", &counter_offer);
        let res = try_execute(deps.as_mut(), _env, info).unwrap();
        assert_eq!(res.messages.len(), 2);
        assert_eq!(
            res.messages[0],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "creator".into(),
                amount: counter_offer,
            })
        );
        assert_eq!(
            res.messages[1],
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "owner".into(),
                amount: collateral,
            })
        );

        // check deleted
        let _ = query_config(deps.as_ref()).unwrap_err();
    }
}
