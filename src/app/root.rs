use std::rc::Rc;
use windows_reactor::*;

use crate::app::home::Home;
use crate::app::page::Page;
use crate::app::placeholder::Placeholder;
use crate::model::media::{self, MediaItem};

pub struct Root {
    page: Page,
    library: Rc<Vec<MediaItem>>,
    pane_open: bool,
}

pub enum Message {
    Select(Option<String>),
    PaneOpenChanged(bool),
}

impl Component for Root {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        Self {
            page: Page::Home,
            library: Rc::new(media::sample_library()),
            pane_open: true,
        }
    }

    fn update(&mut self, message: Message, _context: &ComponentContext<Self>) {
        match message {
            Message::Select(tag) => {
                if let Some(page) = tag.as_deref().and_then(Page::from_tag) {
                    self.page = page;
                }
            }
            Message::PaneOpenChanged(open) => self.pane_open = open,
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        context.window_title("Luminara");
        context.window_visuals(
            WindowVisuals::new()
                .backdrop(WindowBackdrop::Mica)
                .client_size(1100.0, 720.0),
        );

        let menu = Page::ALL.map(|page| {
            KeyedView::new(
                page.tag(),
                NavigationViewItem::new()
                    .tag(page.tag())
                    .is_selected(page == self.page)
                    .slots([
                        SlotView::new(
                            NavigationViewItemSlot::Icon,
                            SymbolIcon::new().symbol(page.icon()),
                        ),
                        SlotView::new(NavigationViewItemSlot::Content, page.title()),
                    ]),
            )
        });

        let content = match self.page {
            Page::Home => View::component::<Home>(Rc::clone(&self.library)),
            other => View::component::<Placeholder>(other),
        };

        NavigationView::new()
            .pane_display_mode(NavigationViewPaneDisplayMode::Left)
            .pane_title("Luminara")
            .is_settings_visible(false)
            // is_pane_open 必须受控:windows-reactor 会在重渲染时把未声明的
            // 双向属性 ClearProperty 清回默认值(true),导致折叠按钮点不动。
            .is_pane_open(self.pane_open)
            .on_is_pane_open_changed(context.callback(Message::PaneOpenChanged))
            .on_selected_tag_changed(context.callback(Message::Select))
            .slots([
                SlotView::collection(NavigationViewSlot::MenuItems, menu),
                SlotView::new(NavigationViewSlot::Content, content),
            ])
    }
}
