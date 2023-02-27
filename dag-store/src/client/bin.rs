mod util;

use crate::util::StatefulTree;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dag_store::client::{Client, MerkleLayer};
use dag_store_types::{
    test::{MerkleToml, MerkleTomlFunctorToken, TomlSimple},
    types::domain::{self, Hash, Header, Id},
};
use recursion_schemes::functor::{AsRefF, Compose, Functor, PartiallyApplied};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt::Display, io};
use tui::{
    backend::{Backend, CrosstermBackend},
    style::{Color, Modifier, Style},
    text::{Spans, Text},
    widgets::{Block, Borders},
    Terminal,
};
use tui_tree_widget::{Tree, TreeItem, TreeItemRender};

type ChildIdx = usize;

struct Layer<F: Functor>(<Compose<MerkleLayer<PartiallyApplied>, F> as Functor>::Layer<ChildIdx>);

impl<F> TreeItemRender for Layer<F>
where
    F: Functor,
    for<'a> Wrapper<<F::RefFunctor<'a> as Functor>::Layer<String>>: Into<Text<'a>>,
    F: AsRefF,
{
    fn as_text(&self) -> tui::text::Text {
        match &self.0 {
            MerkleLayer::Local(hdr, node) => {
                let mut text = Text::from(format!("local: {:?}", hdr));

                let fmtd = <F::RefFunctor<'_> as Functor>::fmap(F::as_ref(&node), |idx| {
                    format!("childidx: {}", idx)
                });

                let t: Text<'_> = Wrapper(fmtd).into();
                text.extend(t);
                text
            }
            MerkleLayer::Remote(hdr) => Text::from(format!("remote: {:?}", hdr)),
            MerkleLayer::ChoseNotToExplore(hdr) => {
                Text::from(format!("chose not to explore: {:?}", hdr))
            }
        }

        // let fmtd = <F::RefFunctor<'_> as Functor>::fmap(F::as_ref(&self.0), |partial| {
        //     format!("{}", partial)
        // });
        // Wrapper(fmtd).into()
    }
}

// from single layer to start, can do multifetch later
fn tree_node_from_layer<F>(hdr: Header, f: F::Layer<Header>) -> TreeItem<Layer<F>>
where
    F: Functor,
    for<'a> Wrapper<<F::RefFunctor<'a> as Functor>::Layer<String>>: Into<Text<'a>>,
    F: AsRefF,
    <F as Functor>::Layer<domain::Id>: Serialize + for<'a> Deserialize<'a>,
    <F as Functor>::Layer<domain::Header>: Clone,
{
    let mut child_nodes = Vec::new();

    let elem = F::fmap(f, |hdr| {
        let idx = child_nodes.len();
        child_nodes.push(TreeItem::new_leaf(Layer(MerkleLayer::Remote(hdr))));
        idx
    });

    TreeItem::new(Layer(MerkleLayer::Local(hdr, elem)), child_nodes)
}

// FIXME: hax for orphan instance rule, specific child idx type
struct Wrapper<T>(T);
impl<'a> Into<Text<'a>> for Wrapper<MerkleToml<String, &'a str>> {
    fn into(self) -> Text<'a> {
        let spans: Vec<Spans<'a>> = match self.0 {
            MerkleToml::Map(xs) => {
                // TODO: append 'map'
                let mut elems = xs.into_iter().collect::<Vec<_>>();
                // produce stable ordering for visualization
                elems.sort();
                elems
                    .iter()
                    .map(|(k, v)| format!("({} -> {}), ", k, v).into())
                    .collect()
            }
            MerkleToml::List(xs) => {
                // TODO: append 'list'
                xs.into_iter().map(|x| x.to_string().into()).collect()
            }
            MerkleToml::Scalar(s) => vec![format!("Scalar: {:?}", s).into()],
        };

        spans.into()
    }
}

struct App<F: Functor> {
    tree: StatefulTree<Layer<F>>,
    client: Client<F>,
}

impl<F> App<F>
where
    F: Functor,
    for<'a> Wrapper<<F::RefFunctor<'a> as Functor>::Layer<String>>: Into<Text<'a>>,
    F: AsRefF,
    <F as Functor>::Layer<domain::Id>: Serialize + for<'a> Deserialize<'a>,
    <F as Functor>::Layer<domain::Header>: Clone,
{
    async fn new(client: Client<F>, root_hash: Hash) -> Result<Self, Box<dyn std::error::Error>> {
        let root = TreeItem::new_leaf(Layer(MerkleLayer::Remote(Header {
            id: Id(0),
            hash: root_hash,
            metadata: "root node (hax)".to_owned(),
        })));

        // TODO: node expand function, initially with one node then multi-expand i think

        Ok(Self {
            tree: StatefulTree::with_items(vec![root]), // TODO need unexpanded root hash I think
            client,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Terminal initialization
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let port = 8088; // TODO: reserve port somehow? idk
                     // spawn svc

    let mut client =
        Client::<MerkleTomlFunctorToken>::build(format!("http://0.0.0.0:{}", port)).await?;

    let t = TomlSimple::List(vec![TomlSimple::Scalar(1), TomlSimple::Scalar(2)]);

    let h = vec![("a".to_string(), t.clone()), ("b".to_string(), t.clone())]
        .into_iter()
        .collect();

    let example = TomlSimple::Map(h);

    let root_hash = client.put_nodes_full(example).await?;

    // App
    let app = App::<MerkleTomlFunctorToken>::new(client, root_hash).await?;
    let res = run_app::<MerkleTomlFunctorToken, _>(&mut terminal, app).await;

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

async fn run_app<F, B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App<F>,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: Functor,
    for<'a> Wrapper<<F::RefFunctor<'a> as Functor>::Layer<String>>: Into<Text<'a>>,
    F: AsRefF,
    <F as Functor>::Layer<domain::Id>: Serialize + for<'a> Deserialize<'a>,
    <F as Functor>::Layer<domain::Header>: Clone,
{
    loop {
        terminal.draw(|f| {
            let area = f.size();

            let items = Tree::new(&app.tree.items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("Tree Widget {:?}", app.tree.state)),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::LightGreen)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(">> ");
            f.render_stateful_widget(&items, area, &mut app.tree.state);
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                // expand partial if selected
                KeyCode::Enter => {
                    let hdr_to_fetch = app.tree.with_selected_leaf(|node| {
                        if let Some(node) = node {
                            match &node.elem.0 {
                                MerkleLayer::Remote(hdr) => Some(hdr.clone()),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    });

                    if let Some(hdr) = hdr_to_fetch {
                        let node = app.client.get_node(hdr.hash).await?;

                        let updated = tree_node_from_layer(hdr, node);

                        app.tree.with_selected_leaf(|node| {
                            if let Some(node) = node {
                                *node = updated;
                            }
                        });
                    }
                }
                KeyCode::Char('\n' | ' ') => app.tree.toggle(),
                KeyCode::Left => app.tree.left(),
                KeyCode::Right => app.tree.right(),
                KeyCode::Down => app.tree.down(),
                KeyCode::Up => app.tree.up(),
                KeyCode::Home => app.tree.first(),
                KeyCode::End => app.tree.last(),
                _ => {}
            }
        }
    }
}
