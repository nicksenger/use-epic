use dioxus::prelude::*;
use futures::{Stream, StreamExt};
use gloo_timers::future::TimeoutFuture;
use use_epic::{combine_epics, use_epic};

fn main() {
    dioxus::web::launch(app);
}

#[derive(Clone, Default, Debug)]
struct State {
    pings: usize,
    pongs: usize,
}

#[derive(Clone)]
enum Action {
    Ping(u32),
    Pong(u32),
    Reset,
}

fn ping_epic(stream: impl Stream<Item = Action>) -> impl Stream<Item = Action> {
    stream
        .filter_map(|action| async move {
            match action {
                Action::Ping(depth) => Some(async move {
                    TimeoutFuture::new(100 * 2_u32.pow(depth)).await;
                    Action::Pong(depth + 1)
                }),
                _ => None,
            }
        })
        .buffer_unordered(usize::MAX)
}

fn pong_epic(stream: impl Stream<Item = Action>) -> impl Stream<Item = Action> {
    stream
        .filter_map(|action| async move {
            match action {
                Action::Pong(depth) => Some(async move {
                    TimeoutFuture::new(100 * 2_u32.pow(depth)).await;
                    Action::Ping(depth + 1)
                }),
                _ => None,
            }
        })
        .buffer_unordered(usize::MAX)
}

fn reset_epic(stream: impl Stream<Item = Action>) -> impl Stream<Item = Action> {
    stream
        .filter(|action| match action {
            Action::Ping(_) | Action::Pong(_) => futures::future::ready(true),
            _ => futures::future::ready(false),
        })
        .chunks(100)
        .map(|_| Action::Reset)
}

fn update(state: &State, action: &Action) -> State {
    let &State { pings, pongs } = state;

    match action {
        Action::Ping(_) => State {
            pings: pings + 1,
            pongs,
        },
        Action::Pong(_) => State {
            pings,
            pongs: pongs + 1,
        },
        Action::Reset => State { pings: 0, pongs: 0 },
    }
}

fn app(cx: Scope) -> Element {
    let store = use_epic(
        &cx,
        combine_epics!(ping_epic, pong_epic, reset_epic),
        update,
    );
    let (pings, pongs) = (store.get_state().pings, store.get_state().pongs);

    cx.render(rsx! {
        div { "Pings: {pings}, Pongs: {pongs}" }
        button { onclick: move |_| store.dispatch(Action::Ping(0)), "ping" }
        button { onclick: move |_| store.dispatch(Action::Pong(0)), "pong" }
    })
}
