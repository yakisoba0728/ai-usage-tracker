import type { LoginInfo, LoginResult, Provider } from "@/lib/types";

export interface AddAccountState {
  selectedProvider: Provider | null;
  info: LoginInfo | null;
  busy: Provider | null;
  pendingLoginProvider: Provider | null;
  sessionFor: Provider | null;
  sessionInput: string;
  error: string | null;
}

export function beginLoginRequest(
  state: AddAccountState,
  provider: Provider,
): { state: AddAccountState; cancelPendingLogin: boolean } {
  return {
    cancelPendingLogin:
      state.pendingLoginProvider != null && state.pendingLoginProvider !== provider,
    state: {
      ...state,
      selectedProvider: provider,
      info: null,
      busy: provider,
      pendingLoginProvider: provider,
      sessionFor: null,
      sessionInput: "",
      error: null,
    },
  };
}

export function receiveLoginInfo(
  state: AddAccountState,
  info: LoginInfo,
): AddAccountState {
  if (state.pendingLoginProvider !== info.provider) return state;
  return {
    ...state,
    info,
    busy: info.provider,
    pendingLoginProvider: info.provider,
    error: null,
  };
}

export function failLoginRequest(
  state: AddAccountState,
  provider: Provider,
  error: string,
): AddAccountState {
  if (state.pendingLoginProvider !== provider) return state;
  return {
    ...state,
    busy: null,
    pendingLoginProvider: null,
    error,
  };
}

export function completeLogin(
  state: AddAccountState,
  result: LoginResult,
): { state: AddAccountState; accepted: boolean; closeDialog: boolean } {
  if (state.pendingLoginProvider !== result.provider) {
    return { state, accepted: false, closeDialog: false };
  }

  return {
    accepted: true,
    closeDialog: result.ok,
    state: {
      ...state,
      info: result.ok ? null : state.info,
      busy: null,
      pendingLoginProvider: null,
      error: result.ok ? null : result.error,
    },
  };
}

export function selectProviderState(
  state: AddAccountState,
  provider: Provider,
): { state: AddAccountState; cancelPendingLogin: boolean } {
  return {
    cancelPendingLogin: state.pendingLoginProvider != null,
    state: {
      ...state,
      selectedProvider: provider,
      info: null,
      busy: null,
      pendingLoginProvider: null,
      sessionFor: null,
      sessionInput: "",
      error: null,
    },
  };
}

export function cancelPendingLoginState(state: AddAccountState): AddAccountState {
  return {
    ...state,
    info: null,
    busy: null,
    pendingLoginProvider: null,
  };
}
