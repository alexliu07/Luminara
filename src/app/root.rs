use std::rc::Rc;
use windows_reactor::*;

use crate::app::home::Home;
use crate::app::library::{Library, LibraryInput};
use crate::app::page::Page;
use crate::app::placeholder::Placeholder;
use crate::model::library::MediaLibrary;
use crate::model::media::{self, MediaItem};
use crate::platform;

pub struct Root {
    page: Page,
    library: Rc<Vec<MediaItem>>,
    /// 已添加的媒体库;本期不持久化,重启清空。
    libraries: Rc<Vec<MediaLibrary>>,
    next_library_id: u64,
    pane_open: bool,
}

pub enum Message {
    Select(Option<String>),
    PaneOpenChanged(bool),
    /// 媒体库页菜单项被点击(回传 label)。
    MenuInvoked(String),
}

impl Component for Root {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        Self {
            page: Page::Home,
            library: Rc::new(media::sample_library()),
            libraries: Rc::new(Vec::new()),
            next_library_id: 1,
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
            Message::MenuInvoked(label) => {
                if label == "从本地添加" {
                    self.add_local_folder();
                }
            }
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
            Page::Library => View::component::<Library>(LibraryInput {
                libraries: Rc::clone(&self.libraries),
                on_menu: context.callback(Message::MenuInvoked),
            }),
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

impl Root {
    /// 弹系统文件夹选择框,选中的文件夹按路径去重后加入媒体库;取消或重复则无变化。
    /// 选择框模态自带消息循环,update 内同步调用安全(LocalSender 队列化投递)。
    fn add_local_folder(&mut self) {
        let Some(path) = platform::pick_folder() else {
            return;
        };
        if self.libraries.iter().any(|lib| lib.path == path) {
            return;
        }
        let library = MediaLibrary::from_path(self.next_library_id, path);
        self.next_library_id += 1;
        Rc::make_mut(&mut self.libraries).push(library);
    }
}
