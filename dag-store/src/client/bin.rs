mod util;

use crate::util::StatefulTree;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dag_store::client::{Client, MerkleLayer};
use dag_store_types::test::MerkleTomlFunctorToken;
use recursion_schemes::functor::{AsRefF, Compose, Functor, PartiallyApplied};
use std::{error::Error, fmt::Display, io};
use tui::{
    backend::{Backend, CrosstermBackend},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders},
    Terminal,
};

use tui_tree_widget::{Tree, TreeItem, TreeItemRender};

type ChildIdx = usize;

struct Layer<F: Functor>(<Compose<MerkleLayer<PartiallyApplied>, F> as Functor>::Layer<ChildIdx>);

impl<F> TreeItemRender for Layer<F>
where
    F: Functor,
    for<'a> <F::RefFunctor<'a> as Functor>::Layer<String>: Display,
    F: AsRefF,
{
    fn as_text(&self) -> tui::text::Text {
        type MLF = MerkleLayer<PartiallyApplied>;
        let fmtd = <<MLF as AsRefF>::RefFunctor<'_> as Functor>::fmap(MLF::as_ref(&self.0), |layer| {
            <F::RefFunctor<'_> as Functor>::fmap(F::as_ref(layer), |idx| format!("{:?}", idx))

        });

        let fmtd = match fmtd {
            MerkleLayer::Local(hdr, l) => format!("local: {:?} => {}", hdr, l),
            MerkleLayer::Remote(hdr) => format!("remote: {:?}", hdr),
            MerkleLayer::ChoseNotToExplore(hdr) => format!("chose not to explore: {:?}", hdr),
        };

        fmtd.into()
    }
}
struct App<F: Functor> {
    tree: StatefulTree<Layer<F>>,
    client: Client<F>,
}

impl<F> App<F>
where
    F: Functor,
    for<'a> <F::RefFunctor<'a> as Functor>::Layer<String>: Display,
    F: AsRefF,
{
    fn new(client: Client<F>) -> Self {
        Self {
            tree: StatefulTree::with_items(vec![]), // TODO need unexpanded root hash I think
            client,
        }
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

    let port = 8098; // TODO: reserve port somehow? idk
                     // spawn svc

    let mut client =
        Client::<MerkleTomlFunctorToken>::build(format!("http://0.0.0.0:{}", port)).await?;

    // App
    let app = App::<MerkleTomlFunctorToken>::new(client);
    let res = run_app::<MerkleTomlFunctorToken, _>(&mut terminal, app);

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

fn run_app<F, B: Backend>(terminal: &mut Terminal<B>, mut app: App<F>) -> io::Result<()>
where
    F: Functor,
    for<'a> <F::RefFunctor<'a> as Functor>::Layer<String>: Display,
    F: AsRefF,
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
                // todo: expand partial if selected
                // KeyCode::Char('a') => {
                //     app.tree.with_selected_leaf(|node| {
                //         if let Some(node) = node {
                //             node.add_child(TreeItem::new_leaf("text"));
                //         }
                //     });
                // }
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
