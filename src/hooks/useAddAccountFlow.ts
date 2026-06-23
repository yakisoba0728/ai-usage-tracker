import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { TFunction } from "i18next";

import { type AddAccountOptionId } from "@/lib/addAccountOptions";
import {
  beginLoginRequest,
  cancelPendingLoginState,
  completeLogin,
  failLoginRequest,
  isCurrentLoginRequest,
  receiveLoginInfo,
  selectProviderState,
  type AddAccountState,
} from "@/lib/addAccountState";
import {
  addSessionKey,
  cancelLogin,
  listAccounts,
  loginOAuth,
  onLoginComplete,
  removeAccount,
  startLogin,
} from "@/lib/ipc";
import { scrubErrorText } from "@/lib/errorScrub";
import type { AccountInfo, Provider } from "@/lib/types";

export interface UseAddAccountFlowArgs {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChanged: () => void;
  t: TFunction;
}

export interface UseAddAccountFlow {
  /** The `addAccountState` machine, mirrored into render state. */
  selectedProvider: Provider | null;
  info: AddAccountState["info"];
  busy: Provider | null;
  sessionFor: Provider | null;
  sessionInput: string;
  error: string | null;
  /** Persisted accounts list (for AddedAccountsList). */
  accounts: AccountInfo[];
  /** Intents. */
  chooseProvider: (provider: Provider) => void;
  runOption: (provider: Provider, optionId: AddAccountOptionId) => Promise<void>;
  setSessionInput: (value: string) => void;
  submitSession: () => Promise<void>;
  remove: (id: string) => Promise<void>;
}

/**
 * Owns the AddAccountDialog login machine: the `addAccountState` transitions
 * mirrored into render state, the `loginRequestRef`/`pendingLoginRef`
 * race-guards, the three async login flows, the `login-complete` subscription,
 * and the `listAccounts`/`removeAccount` persistence. The dialog stays
 * presentational, wiring this flat state + intents into the view pieces.
 *
 * The state apparatus (`stateRef`/`renderedState`/`currentState`/`applyState`)
 * is preserved verbatim from the original component so the subscribe/unsubscribe
 * lifecycle of `onLoginComplete` — driven by `currentState`'s `[renderedState]`
 * dependency — stays identical. The request-id claim-before-await /
 * check-after-await pattern (via `isCurrentLoginRequest`) is the stale-response
 * guard and is unchanged.
 */
export function useAddAccountFlow({
  open,
  onOpenChange,
  onChanged,
  t,
}: UseAddAccountFlowArgs): UseAddAccountFlow {
  const [selectedProvider, setSelectedProvider] = useState<Provider | null>(null);
  const [info, setInfo] = useState<AddAccountState["info"]>(null);
  const [busy, setBusy] = useState<Provider | null>(null);
  const [pendingLoginProvider, setPendingLoginProvider] = useState<Provider | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<AccountInfo[]>([]);
  const [sessionFor, setSessionFor] = useState<Provider | null>(null);
  const [sessionInput, setSessionInput] = useState("");
  const loginRequestRef = useRef(0);
  const pendingLoginRef = useRef<Provider | null>(null);
  const stateRef = useRef<AddAccountState | null>(null);

  const renderedState = useCallback((): AddAccountState => ({
    selectedProvider,
    info,
    busy,
    pendingLoginProvider,
    sessionFor,
    sessionInput,
    error,
  }), [busy, error, info, pendingLoginProvider, selectedProvider, sessionFor, sessionInput]);

  useEffect(() => {
    stateRef.current = renderedState();
  }, [renderedState]);

  const currentState = useCallback(
    (): AddAccountState => stateRef.current ?? renderedState(),
    [renderedState],
  );

  const applyState = useCallback((next: AddAccountState) => {
    stateRef.current = next;
    setSelectedProvider(next.selectedProvider);
    setInfo(next.info);
    setBusy(next.busy);
    pendingLoginRef.current = next.pendingLoginProvider;
    setPendingLoginProvider(next.pendingLoginProvider);
    setSessionFor(next.sessionFor);
    setSessionInput(next.sessionInput);
    setError(next.error);
  }, []);

  const cancelPendingLogin = useCallback(() => {
    loginRequestRef.current += 1;
    applyState(cancelPendingLoginState(currentState()));
    void cancelLogin().catch(() => {});
  }, [applyState, currentState]);

  async function load() {
    try {
      setAccounts(await listAccounts());
    } catch (e) {
      setAccounts([]);
      setError(scrubErrorText(String(e)));
    }
  }

  useEffect(() => {
    if (open) {
      void load();
      return;
    }
    if (pendingLoginRef.current != null) {
      void cancelLogin().catch(() => {});
      pendingLoginRef.current = null;
    }
    loginRequestRef.current += 1;
    setSelectedProvider(null);
    setInfo(null);
    setError(null);
    setBusy(null);
    setPendingLoginProvider(null);
    setSessionFor(null);
    setSessionInput("");
  }, [open]);

  useEffect(() => {
    const unP = onLoginComplete((r) => {
      const result = completeLogin(currentState(), r);
      if (!result.accepted) return;
      applyState(
        result.state.error == null && !r.ok
          ? { ...result.state, error: t("addAccount.loginFailed") }
          : result.state,
      );
      if (result.closeDialog) {
        void load();
        onChanged();
        onOpenChange(false);
      }
    });
    return () => {
      void unP.then((u) => u());
    };
  }, [applyState, currentState, onChanged, onOpenChange, t]);

  function chooseProvider(provider: Provider) {
    const result = selectProviderState(currentState(), provider);
    applyState(result.state);
    if (result.cancelPendingLogin) {
      loginRequestRef.current += 1;
      void cancelLogin().catch(() => {});
    }
  }

  async function startBrowserOAuth(provider: Provider) {
    const requestId = loginRequestRef.current + 1;
    loginRequestRef.current = requestId;
    const pending = beginLoginRequest(currentState(), provider);
    applyState(pending.state);
    if (pending.cancelPendingLogin) {
      void cancelLogin().catch(() => {});
    }
    try {
      const url = await loginOAuth(provider);
      if (!isCurrentLoginRequest(requestId, loginRequestRef.current)) return;
      applyState(
        receiveLoginInfo(pending.state, {
          provider,
          verification_url: url,
          user_code: "",
          expires_in: 300,
        }),
      );
      await openUrl(url);
    } catch (e) {
      if (isCurrentLoginRequest(requestId, loginRequestRef.current)) {
        applyState(failLoginRequest(pending.state, provider, scrubErrorText(String(e))));
      }
    }
  }

  async function startDeviceCode(provider: Provider) {
    const requestId = loginRequestRef.current + 1;
    loginRequestRef.current = requestId;
    const pending = beginLoginRequest(currentState(), provider);
    applyState(pending.state);
    if (pending.cancelPendingLogin) {
      void cancelLogin().catch(() => {});
    }
    try {
      const i = await startLogin(provider);
      if (!isCurrentLoginRequest(requestId, loginRequestRef.current)) return;
      applyState(receiveLoginInfo(pending.state, i));
      await openUrl(i.verification_url);
    } catch (e) {
      if (isCurrentLoginRequest(requestId, loginRequestRef.current)) {
        applyState(failLoginRequest(pending.state, provider, scrubErrorText(String(e))));
      }
    }
  }

  async function runOption(provider: Provider, optionId: AddAccountOptionId) {
    if (pendingLoginRef.current != null) cancelPendingLogin();
    setError(null);
    setInfo(null);
    setSessionFor(null);
    setSessionInput("");

    if (optionId === "session-key") {
      setSessionFor(provider);
      return;
    }
    if (optionId === "browser-oauth") {
      await startBrowserOAuth(provider);
      return;
    }
    if (optionId === "device-code") {
      await startDeviceCode(provider);
      return;
    }

    onChanged();
    onOpenChange(false);
  }

  async function submitSession() {
    const provider = sessionFor;
    if (!provider || !sessionInput.trim()) return;
    setError(null);
    setBusy(provider);
    try {
      await addSessionKey(provider, sessionInput.trim());
      setSessionFor(null);
      setSessionInput("");
      await load();
      onChanged();
    } catch (e) {
      setError(scrubErrorText(String(e)));
    } finally {
      setBusy(null);
    }
  }

  async function remove(id: string) {
    try {
      const removed = await removeAccount(id);
      if (!removed) {
        setError(t("addAccount.removeFailed"));
        return;
      }
    } catch (e) {
      setError(scrubErrorText(String(e)));
      return;
    }
    await load();
    onChanged();
  }

  return {
    selectedProvider,
    info,
    busy,
    sessionFor,
    sessionInput,
    error,
    accounts,
    chooseProvider,
    runOption,
    setSessionInput,
    submitSession,
    remove,
  };
}
