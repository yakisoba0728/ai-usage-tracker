export type AccountActionKind = "refresh" | "anchor";
export type AccountActionCompletionStatus = "success" | "error";
export type AccountActionStatus = "pending" | AccountActionCompletionStatus;
export type AccountActionState = Readonly<
  Record<string, Readonly<Partial<Record<AccountActionKind, AccountActionStatus>>>>
>;

export type StartAccountActionResult = {
  readonly state: AccountActionState;
  readonly started: boolean;
};

export function startAccountAction(
  state: AccountActionState,
  serviceId: string,
  kind: AccountActionKind,
): StartAccountActionResult {
  if (state[serviceId]?.[kind] === "pending") {
    return { state, started: false };
  }

  return {
    state: {
      ...state,
      [serviceId]: {
        ...state[serviceId],
        [kind]: "pending",
      },
    },
    started: true,
  };
}

export function getAccountAction(
  state: AccountActionState,
  serviceId: string,
  kind: AccountActionKind,
): AccountActionStatus | null {
  return state[serviceId]?.[kind] ?? null;
}

export function isAccountActionPending(
  state: AccountActionState,
  serviceId: string,
  kind: AccountActionKind,
): boolean {
  return getAccountAction(state, serviceId, kind) === "pending";
}

export function finishAccountAction(
  state: AccountActionState,
  serviceId: string,
  kind: AccountActionKind,
  status: AccountActionCompletionStatus,
): AccountActionState {
  if (!isAccountActionPending(state, serviceId, kind)) {
    return state;
  }

  return {
    ...state,
    [serviceId]: {
      ...state[serviceId],
      [kind]: status,
    },
  };
}

export function clearAccountAction(
  state: AccountActionState,
  serviceId: string,
  kind: AccountActionKind,
): AccountActionState {
  if (state[serviceId]?.[kind] === undefined) {
    return state;
  }

  const nextServiceActions = { ...state[serviceId] };
  delete nextServiceActions[kind];

  if (Object.keys(nextServiceActions).length > 0) {
    return {
      ...state,
      [serviceId]: nextServiceActions,
    };
  }

  const nextState = { ...state };
  delete nextState[serviceId];
  return nextState;
}
