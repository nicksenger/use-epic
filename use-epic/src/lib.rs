use dioxus::core::ScopeState;
use dioxus::hooks::{use_state, UseState};
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::future;
use futures::stream::select;
use futures::{SinkExt, Stream, StreamExt};

use std::marker::{Send, Sync};
use std::pin::Pin;

pub struct Store<'a, State: 'static, Action> {
    state: &'a UseState<State>,
    dispatcher: &'a UnboundedSender<Action>,
}

impl<'a, State: 'static, Action> Clone for Store<'a, State, Action> {
    fn clone(&self) -> Self {
        Self {
            state: self.state,
            dispatcher: self.dispatcher,
        }
    }
}

impl<'a, State, Action> Copy for Store<'a, State, Action> {}

impl<'a, State: 'static, Action> Store<'a, State, Action> {
    pub fn get_state(&self) -> &'a State {
        self.state.get()
    }

    pub fn dispatch(&self, action: Action) {
        let _ = self.dispatcher.clone().start_send(action);
    }
}

pub fn use_epic<State, Action>(
    cx: &ScopeState,
    epic: impl FnOnce(UnboundedReceiver<Action>) -> Pin<Box<dyn Stream<Item = Action> + 'static>>,
    update: impl Fn(&State, &Action) -> State + 'static,
) -> Store<State, Action>
where
    State: std::fmt::Debug + Default + 'static,
    Action: Send + Sync + 'static,
{
    let state = use_state(cx, || State::default());
    let owned_state = state.to_owned();
    let dispatcher = cx.use_hook(move |_| {
        let (mut dispatch_tx, dispatch_rx): (UnboundedSender<Action>, UnboundedReceiver<Action>) =
            mpsc::unbounded();
        let (mut epic_tx, rx) = mpsc::unbounded();
        let epic_rx = epic(rx);

        let dispatcher = dispatch_tx.clone();
        let async_state = owned_state.clone();

        cx.push_future(async move {
            enum Kind<Action> {
                Dispatched(Action),
                Epic(Action),
            }

            let actions = select(dispatch_rx.map(Kind::Dispatched), epic_rx.map(Kind::Epic));
            pin_utils::pin_mut!(actions);

            while let Some(action) = actions.next().await {
                match action {
                    Kind::Dispatched(action) => {
                        async_state.modify(|state| update(state, &action));
                        let _ = epic_tx.send(action).await;
                    }
                    Kind::Epic(action) => {
                        let _ = dispatch_tx.send(action).await;
                    }
                }
            }
        });

        dispatcher
    });

    Store { state, dispatcher }
}

pub fn combine_epics<
    Action: Clone + Send + Sync + 'static,
    A: Stream<Item = Action> + 'static,
    B: Stream<Item = Action> + 'static,
>(
    a: impl FnOnce(UnboundedReceiver<Action>) -> A + 'static,
    b: impl FnOnce(UnboundedReceiver<Action>) -> B + 'static,
) -> impl FnOnce(UnboundedReceiver<Action>) -> Pin<Box<dyn Stream<Item = Action>>> {
    |in_stream| {
        let ((tx_a, rx_a), (tx_b, rx_b)) = (mpsc::unbounded(), mpsc::unbounded());
        let (rx_a, rx_b) = (a(rx_a), b(rx_b));

        let (tx, rx) = (tx_a.fanout(tx_b), select(rx_a, rx_b).map(Option::Some));

        let pipe = in_stream.scan(tx, |tx, action| {
            let _ = tx.start_send_unpin(action);
            future::ready(Some(None))
        });

        Box::pin(select(pipe, rx).filter_map(future::ready))
    }
}

#[macro_export]
macro_rules! combine_epics {
    ($a:expr, $b:expr, $($rest:expr),*) => {
        combine_epics!(combine_epics($a, $b), $($rest),*)
    };
    ($a:expr, $b:expr) => {
        combine_epics($a, $b)
    };
}
