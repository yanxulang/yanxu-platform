//! 将言台无句柄语义树确定性转换为 AccessKit 原生树。

use crate::accessibility::{AccessibilityState, SemanticNode};
use crate::data::Data;
use crate::sync::RecoverMutex;
use accesskit::{
    Action, ActivationHandler, AriaCurrent, CustomAction, Invalid, Node, NodeId, Orientation, Rect,
    Role, Toggled, Tree, TreeId, TreeUpdate,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub(crate) const WINDOW_ROOT_ID: NodeId = NodeId(0);

const CUSTOM_SELECT: i32 = 1;
const CUSTOM_DESELECT: i32 = 2;
const CUSTOM_COPY: i32 = 3;
const CUSTOM_CUT: i32 = 4;
const CUSTOM_PASTE: i32 = 5;
const MAX_NATIVE_SCALE_FACTOR: f64 = 1_024.0;
const MAX_EXACT_F64_INTEGER: i64 = 9_007_199_254_740_992;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NativeTreeSnapshot {
    title: String,
    physical_size: [u32; 2],
    scale_factor: f64,
    accessibility: AccessibilityState,
}

impl NativeTreeSnapshot {
    pub(crate) fn new(
        title: &str,
        physical_size: [u32; 2],
        scale_factor: f64,
        accessibility: &AccessibilityState,
    ) -> Self {
        Self {
            title: title.to_owned(),
            physical_size,
            scale_factor,
            accessibility: accessibility.clone(),
        }
    }

    pub(crate) fn replace(
        &mut self,
        title: &str,
        physical_size: [u32; 2],
        scale_factor: f64,
        accessibility: &AccessibilityState,
    ) {
        self.title.clear();
        self.title.push_str(title);
        self.physical_size = physical_size;
        self.scale_factor = scale_factor;
        self.accessibility.clone_from(accessibility);
    }

    #[must_use]
    pub(crate) fn full_update(&self) -> TreeUpdate {
        full_tree_update(
            &self.title,
            self.physical_size,
            self.scale_factor,
            &self.accessibility,
        )
    }
}

pub(crate) struct NativeActivationHandler {
    snapshot: Arc<Mutex<NativeTreeSnapshot>>,
}

impl NativeActivationHandler {
    pub(crate) fn new(snapshot: Arc<Mutex<NativeTreeSnapshot>>) -> Self {
        Self { snapshot }
    }
}

impl ActivationHandler for NativeActivationHandler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(self.snapshot.lock_recover().full_update())
    }
}

const ROLE_MAP: &[(&str, Role)] = &[
    ("窗口", Role::Window),
    ("组", Role::Group),
    ("面板", Role::Pane),
    ("控件", Role::Unknown),
    ("文字", Role::Label),
    ("标题", Role::Heading),
    ("按钮", Role::Button),
    ("链接", Role::Link),
    ("复选框", Role::CheckBox),
    ("单选框", Role::RadioButton),
    ("切换", Role::Switch),
    ("输入框", Role::TextInput),
    ("多行输入框", Role::MultilineTextInput),
    ("密码框", Role::PasswordInput),
    ("组合框", Role::ComboBox),
    ("列表", Role::List),
    ("列表项", Role::ListItem),
    ("树", Role::Tree),
    ("树项", Role::TreeItem),
    ("标签页", Role::TabList),
    ("标签", Role::Tab),
    ("菜单栏", Role::MenuBar),
    ("菜单", Role::Menu),
    ("菜单项", Role::MenuItem),
    ("弹出层", Role::Group),
    ("对话框", Role::Dialog),
    ("滑块", Role::Slider),
    ("进度条", Role::ProgressIndicator),
    ("滚动条", Role::ScrollBar),
    ("滚动容器", Role::ScrollView),
    ("分割面板", Role::Splitter),
    ("工具栏", Role::Toolbar),
    ("画布", Role::Canvas),
    ("图片", Role::Image),
    ("表格", Role::Table),
    ("行", Role::Row),
    ("单元格", Role::Cell),
    ("分隔符", Role::Splitter),
];

/// 构造包含稳定窗口根节点的完整原生树更新。
pub(crate) fn full_tree_update(
    title: &str,
    physical_size: [u32; 2],
    scale_factor: f64,
    accessibility: &AccessibilityState,
) -> TreeUpdate {
    let scale_factor = normalized_scale_factor(scale_factor);
    let mut window = Node::new(Role::Window);
    if !title.is_empty() {
        window.set_label(title);
    }
    window.set_bounds(Rect {
        x0: 0.0,
        y0: 0.0,
        x1: f64::from(physical_size[0]),
        y1: f64::from(physical_size[1]),
    });

    let mut nodes = Vec::with_capacity(accessibility.node_count().saturating_add(1));
    if let Some(tree) = accessibility.tree() {
        window.push_child(node_id(tree.root().id()));
    }
    nodes.push((WINDOW_ROOT_ID, window));
    if let Some(tree) = accessibility.tree() {
        append_node(&mut nodes, tree.root(), scale_factor);
    }

    let mut tree = Tree::new(WINDOW_ROOT_ID);
    tree.toolkit_name = Some("言台".to_owned());
    tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").to_owned());
    TreeUpdate {
        nodes,
        tree: Some(tree),
        tree_id: TreeId::ROOT,
        focus: accessibility.focused().map_or(WINDOW_ROOT_ID, node_id),
    }
}

fn normalized_scale_factor(scale_factor: f64) -> f64 {
    if scale_factor.is_finite() && scale_factor > 0.0 && scale_factor <= MAX_NATIVE_SCALE_FACTOR {
        scale_factor
    } else {
        1.0
    }
}

fn append_node(nodes: &mut Vec<(NodeId, Node)>, semantic: &SemanticNode, scale_factor: f64) {
    let mut native = Node::new(native_role(semantic.role()));
    if !semantic.name().is_empty() {
        if semantic.role() == "文字" && matches!(semantic.value(), Data::Nil) {
            native.set_value(semantic.name());
        } else {
            native.set_label(semantic.name());
        }
    }
    if !semantic.description().is_empty() {
        native.set_description(semantic.description());
    }
    apply_value(&mut native, semantic.value());
    apply_states(&mut native, semantic.states());
    apply_actions(
        &mut native,
        semantic.role(),
        semantic.actions(),
        semantic.states(),
    );

    let [x, y, width, height] = semantic.bounds();
    native.set_bounds(Rect {
        x0: x * scale_factor,
        y0: y * scale_factor,
        x1: (x + width) * scale_factor,
        y1: (y + height) * scale_factor,
    });
    native.set_children(
        semantic
            .children()
            .iter()
            .map(|child| node_id(child.id()))
            .collect::<Vec<_>>(),
    );
    nodes.push((node_id(semantic.id()), native));
    for child in semantic.children() {
        append_node(nodes, child, scale_factor);
    }
}

fn native_role(role: &str) -> Role {
    ROLE_MAP
        .iter()
        .find_map(|(name, native)| (*name == role).then_some(*native))
        .unwrap_or(Role::Unknown)
}

fn node_id(id: i64) -> NodeId {
    NodeId(u64::try_from(id).expect("validated semantic node IDs are positive"))
}

fn apply_value(node: &mut Node, value: &Data) {
    match value {
        Data::Nil => {}
        Data::Bool(value) => node.set_value(if *value { "true" } else { "false" }),
        Data::Integer(value) => {
            node.set_value(value.to_string());
            if (-MAX_EXACT_F64_INTEGER..=MAX_EXACT_F64_INTEGER).contains(value) {
                node.set_numeric_value(*value as f64);
            }
        }
        Data::Number(value) => {
            node.set_value(value.to_string());
            node.set_numeric_value(*value);
        }
        Data::String(value) => node.set_value(value.as_str()),
        Data::Bytes(_) | Data::Array(_) | Data::Map(_) | Data::Resource(_) | Data::Callback(_) => {
            debug_assert!(false, "semantic values are validated as scalars");
        }
    }
}

fn apply_states(node: &mut Node, states: &BTreeMap<String, Data>) {
    if state_bool(states, "启用") == Some(false) {
        node.set_disabled();
    }
    if state_bool(states, "可见") == Some(false) {
        node.set_hidden();
    }
    if state_bool(states, "忙碌") == Some(true) {
        node.set_busy();
    }
    if state_bool(states, "只读") == Some(true) {
        node.set_read_only();
    }
    if state_bool(states, "必填") == Some(true) {
        node.set_required();
    }
    if state_bool(states, "无效") == Some(true) {
        node.set_invalid(Invalid::True);
    }
    if let Some(selected) = state_bool(states, "选中") {
        node.set_selected(selected);
    }
    if let Some(current) = state_bool(states, "当前") {
        node.set_aria_current(if current {
            AriaCurrent::True
        } else {
            AriaCurrent::False
        });
    }
    if let Some(expanded) = state_bool(states, "展开") {
        node.set_expanded(expanded);
    }
    if state_bool(states, "多选") == Some(true) {
        node.set_multiselectable();
    }
    if state_bool(states, "模态") == Some(true) {
        node.set_modal();
    }

    let toggled = if state_bool(states, "混合") == Some(true) {
        Some(Toggled::Mixed)
    } else if let Some(checked) = state_bool(states, "已检查") {
        Some(checked.into())
    } else {
        state_bool(states, "按下").map(Into::into)
    };
    if let Some(toggled) = toggled {
        node.set_toggled(toggled);
    }

    if let Some(Data::String(orientation)) = states.get("方向") {
        node.set_orientation(match orientation.as_str() {
            "横向" => Orientation::Horizontal,
            "纵向" => Orientation::Vertical,
            _ => return,
        });
    }
    if let Some(Data::Integer(level)) = states.get("级别")
        && let Ok(level) = usize::try_from(*level)
    {
        node.set_level(level);
    }
}

fn apply_actions(node: &mut Node, role: &str, actions: &[String], states: &BTreeMap<String, Data>) {
    for action in actions {
        match action.as_str() {
            "聚焦" => node.add_action(Action::Focus),
            "点击" => node.add_action(Action::Click),
            "设置值" => node.add_action(Action::SetValue),
            "选择" if matches!(role, "输入框" | "多行输入框" | "密码框") => {
                node.add_action(Action::SetTextSelection);
            }
            "选择" if actions.iter().any(|action| action == "点击") => {
                add_custom_action(node, CUSTOM_SELECT, "选择");
            }
            "选择" => node.add_action(Action::Click),
            "取消选择" => add_custom_action(node, CUSTOM_DESELECT, "取消选择"),
            "复制" => add_custom_action(node, CUSTOM_COPY, "复制"),
            "剪切" => add_custom_action(node, CUSTOM_CUT, "剪切"),
            "粘贴" => add_custom_action(node, CUSTOM_PASTE, "粘贴"),
            "展开" => node.add_action(Action::Expand),
            "折叠" => node.add_action(Action::Collapse),
            "增加" => node.add_action(Action::Increment),
            "减少" => node.add_action(Action::Decrement),
            "滚动" => add_scroll_actions(node, states),
            "滚动到" => node.add_action(Action::ScrollIntoView),
            "显示菜单" => node.add_action(Action::ShowContextMenu),
            _ => debug_assert!(false, "semantic actions are validated before conversion"),
        }
    }
}

fn add_custom_action(node: &mut Node, id: i32, description: &str) {
    node.add_action(Action::CustomAction);
    node.push_custom_action(CustomAction {
        id,
        description: description.into(),
    });
}

fn add_scroll_actions(node: &mut Node, states: &BTreeMap<String, Data>) {
    match states.get("方向") {
        Some(Data::String(value)) if value == "横向" => {
            node.add_action(Action::ScrollLeft);
            node.add_action(Action::ScrollRight);
        }
        Some(Data::String(value)) if value == "纵向" => {
            node.add_action(Action::ScrollUp);
            node.add_action(Action::ScrollDown);
        }
        _ => {
            node.add_action(Action::ScrollLeft);
            node.add_action(Action::ScrollRight);
            node.add_action(Action::ScrollUp);
            node.add_action(Action::ScrollDown);
        }
    }
}

fn state_bool(states: &BTreeMap<String, Data>, name: &str) -> Option<bool> {
    match states.get(name) {
        Some(Data::Bool(value)) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::{SEMANTIC_ACTIONS, SEMANTIC_ROLES, SemanticTree};

    fn semantic_node(
        id: i64,
        role: &str,
        value: Data,
        states: BTreeMap<String, Data>,
        actions: &[&str],
        bounds: [f64; 4],
        children: Vec<Data>,
    ) -> Data {
        Data::map([
            ("编号", Data::Integer(id)),
            ("角色", Data::String(role.to_owned())),
            ("名称", Data::String(format!("节点{id}"))),
            ("描述", Data::String(format!("说明{id}"))),
            ("值", value),
            ("状态", Data::Map(states)),
            (
                "操作",
                Data::Array(
                    actions
                        .iter()
                        .map(|action| Data::String((*action).to_owned()))
                        .collect(),
                ),
            ),
            (
                "边界",
                Data::Array(bounds.into_iter().map(Data::Number).collect()),
            ),
            ("子", Data::Array(children)),
        ])
    }

    fn state_with(tree: Data) -> AccessibilityState {
        let mut state = AccessibilityState::default();
        state
            .replace(Some(SemanticTree::validate(&tree).unwrap()))
            .unwrap();
        state
    }

    fn converted_node(update: &TreeUpdate, id: u64) -> &Node {
        &update
            .nodes
            .iter()
            .find(|(node_id, _)| *node_id == NodeId(id))
            .unwrap()
            .1
    }

    #[test]
    fn maps_every_protocol_role_without_dropping_nodes() {
        assert_eq!(ROLE_MAP.len(), SEMANTIC_ROLES.len());
        assert_eq!(
            ROLE_MAP.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            SEMANTIC_ROLES
        );
        for (index, (role, expected)) in ROLE_MAP.iter().enumerate() {
            let id = i64::try_from(index).unwrap() + 1;
            let state = state_with(semantic_node(
                id,
                role,
                Data::Nil,
                BTreeMap::new(),
                &[],
                [0.0, 0.0, 1.0, 1.0],
                Vec::new(),
            ));
            let update = full_tree_update("窗口", [20, 10], 1.0, &state);
            let converted = converted_node(&update, u64::try_from(id).unwrap());
            assert_eq!(converted.role(), *expected);
            if *role == "文字" {
                assert_eq!(converted.value(), Some(format!("节点{id}").as_str()));
                assert_eq!(converted.label(), None);
            }
        }
    }

    #[test]
    fn builds_a_scaled_full_tree_with_stable_root_and_focus() {
        let focused = semantic_node(
            2,
            "按钮",
            Data::Bool(true),
            BTreeMap::from([
                ("启用".to_owned(), Data::Bool(true)),
                ("可见".to_owned(), Data::Bool(true)),
                ("可聚焦".to_owned(), Data::Bool(true)),
                ("焦点".to_owned(), Data::Bool(true)),
                ("忙碌".to_owned(), Data::Bool(true)),
                ("按下".to_owned(), Data::Bool(false)),
            ]),
            &["点击", "聚焦"],
            [10.0, 20.0, 30.0, 40.0],
            Vec::new(),
        );
        let state = state_with(semantic_node(
            1,
            "面板",
            Data::Nil,
            BTreeMap::new(),
            &[],
            [0.0, 0.0, 100.0, 80.0],
            vec![focused],
        ));
        let update = full_tree_update("主窗口", [300, 200], 2.0, &state);

        assert_eq!(update.tree_id, TreeId::ROOT);
        assert_eq!(update.focus, NodeId(2));
        let tree = update.tree.as_ref().unwrap();
        assert_eq!(tree.root, WINDOW_ROOT_ID);
        assert_eq!(tree.toolkit_name.as_deref(), Some("言台"));
        assert_eq!(
            tree.toolkit_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        let root = converted_node(&update, 0);
        assert_eq!(root.label(), Some("主窗口"));
        assert_eq!(root.bounds(), Some(Rect::new(0.0, 0.0, 300.0, 200.0)));
        assert_eq!(root.children(), &[NodeId(1)]);
        let panel = converted_node(&update, 1);
        assert_eq!(panel.children(), &[NodeId(2)]);
        let button = converted_node(&update, 2);
        assert_eq!(button.bounds(), Some(Rect::new(20.0, 40.0, 80.0, 120.0)));
        assert_eq!(button.label(), Some("节点2"));
        assert_eq!(button.description(), Some("说明2"));
        assert_eq!(button.value(), Some("true"));
        assert!(button.is_busy());
        assert_eq!(button.toggled(), Some(Toggled::False));
        assert!(button.supports_action(Action::Click));
        assert!(button.supports_action(Action::Focus));
    }

    #[test]
    fn maps_all_state_families_to_native_properties() {
        let disabled = semantic_node(
            2,
            "按钮",
            Data::Nil,
            BTreeMap::from([
                ("启用".to_owned(), Data::Bool(false)),
                ("可见".to_owned(), Data::Bool(false)),
            ]),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let input = semantic_node(
            3,
            "输入框",
            Data::String("值".to_owned()),
            BTreeMap::from([
                ("只读".to_owned(), Data::Bool(true)),
                ("必填".to_owned(), Data::Bool(true)),
                ("无效".to_owned(), Data::Bool(true)),
            ]),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let item = semantic_node(
            4,
            "列表项",
            Data::Nil,
            BTreeMap::from([
                ("选中".to_owned(), Data::Bool(false)),
                ("当前".to_owned(), Data::Bool(true)),
            ]),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let check = semantic_node(
            5,
            "复选框",
            Data::Nil,
            BTreeMap::from([
                ("已检查".to_owned(), Data::Bool(false)),
                ("混合".to_owned(), Data::Bool(true)),
            ]),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let combo = semantic_node(
            6,
            "组合框",
            Data::Integer(7),
            BTreeMap::from([("展开".to_owned(), Data::Bool(false))]),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let list = semantic_node(
            7,
            "列表",
            Data::Nil,
            BTreeMap::from([("多选".to_owned(), Data::Bool(true))]),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let dialog = semantic_node(
            8,
            "对话框",
            Data::Nil,
            BTreeMap::from([("模态".to_owned(), Data::Bool(true))]),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let slider = semantic_node(
            9,
            "滑块",
            Data::Number(2.5),
            BTreeMap::from([("方向".to_owned(), Data::String("横向".to_owned()))]),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let heading = semantic_node(
            10,
            "标题",
            Data::Nil,
            BTreeMap::from([("级别".to_owned(), Data::Integer(3))]),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let state = state_with(semantic_node(
            1,
            "面板",
            Data::Nil,
            BTreeMap::new(),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            vec![
                disabled, input, item, check, combo, list, dialog, slider, heading,
            ],
        ));
        let update = full_tree_update("", [1, 1], 1.0, &state);

        let disabled = converted_node(&update, 2);
        assert!(disabled.is_disabled());
        assert!(disabled.is_hidden());
        let input = converted_node(&update, 3);
        assert!(input.is_read_only());
        assert!(input.is_required());
        assert_eq!(input.invalid(), Some(Invalid::True));
        assert_eq!(input.value(), Some("值"));
        let item = converted_node(&update, 4);
        assert_eq!(item.is_selected(), Some(false));
        assert_eq!(item.aria_current(), Some(AriaCurrent::True));
        assert_eq!(converted_node(&update, 5).toggled(), Some(Toggled::Mixed));
        let combo = converted_node(&update, 6);
        assert_eq!(combo.is_expanded(), Some(false));
        assert_eq!(combo.numeric_value(), Some(7.0));
        assert!(converted_node(&update, 7).is_multiselectable());
        assert!(converted_node(&update, 8).is_modal());
        let slider = converted_node(&update, 9);
        assert_eq!(slider.orientation(), Some(Orientation::Horizontal));
        assert_eq!(slider.numeric_value(), Some(2.5));
        assert_eq!(converted_node(&update, 10).level(), Some(3));
    }

    #[test]
    fn maps_every_protocol_action_to_a_native_action() {
        let cases: &[(&str, &str, BTreeMap<String, Data>, Action)] = &[
            (
                "聚焦",
                "按钮",
                BTreeMap::from([("可聚焦".to_owned(), Data::Bool(true))]),
                Action::Focus,
            ),
            ("点击", "按钮", BTreeMap::new(), Action::Click),
            ("设置值", "输入框", BTreeMap::new(), Action::SetValue),
            ("选择", "输入框", BTreeMap::new(), Action::SetTextSelection),
            ("取消选择", "列表项", BTreeMap::new(), Action::CustomAction),
            ("复制", "输入框", BTreeMap::new(), Action::CustomAction),
            ("剪切", "输入框", BTreeMap::new(), Action::CustomAction),
            ("粘贴", "输入框", BTreeMap::new(), Action::CustomAction),
            ("展开", "组合框", BTreeMap::new(), Action::Expand),
            ("折叠", "组合框", BTreeMap::new(), Action::Collapse),
            ("增加", "滑块", BTreeMap::new(), Action::Increment),
            ("减少", "滑块", BTreeMap::new(), Action::Decrement),
            ("滚动", "列表", BTreeMap::new(), Action::ScrollDown),
            ("滚动到", "图片", BTreeMap::new(), Action::ScrollIntoView),
            ("显示菜单", "按钮", BTreeMap::new(), Action::ShowContextMenu),
        ];
        assert_eq!(cases.len(), SEMANTIC_ACTIONS.len());
        for (index, (action, role, states, expected)) in cases.iter().enumerate() {
            assert_eq!(*action, SEMANTIC_ACTIONS[index]);
            let state = state_with(semantic_node(
                1,
                role,
                Data::Nil,
                states.clone(),
                &[*action],
                [0.0, 0.0, 1.0, 1.0],
                Vec::new(),
            ));
            let update = full_tree_update("", [1, 1], 1.0, &state);
            assert!(converted_node(&update, 1).supports_action(*expected));
        }
    }

    #[test]
    fn preserves_distinct_click_and_select_actions_and_scroll_directions() {
        let tab = semantic_node(
            2,
            "标签",
            Data::Nil,
            BTreeMap::new(),
            &["点击", "选择", "取消选择"],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let scroll = semantic_node(
            3,
            "列表",
            Data::Nil,
            BTreeMap::new(),
            &["滚动"],
            [0.0, 0.0, 1.0, 1.0],
            Vec::new(),
        );
        let state = state_with(semantic_node(
            1,
            "面板",
            Data::Nil,
            BTreeMap::new(),
            &[],
            [0.0, 0.0, 1.0, 1.0],
            vec![tab, scroll],
        ));
        let update = full_tree_update("", [1, 1], 1.0, &state);
        let tab = converted_node(&update, 2);
        assert!(tab.supports_action(Action::Click));
        assert!(tab.supports_action(Action::CustomAction));
        assert_eq!(
            tab.custom_actions()
                .iter()
                .map(|action| (action.id, action.description.as_ref()))
                .collect::<Vec<_>>(),
            vec![(CUSTOM_SELECT, "选择"), (CUSTOM_DESELECT, "取消选择")]
        );
        let scroll = converted_node(&update, 3);
        assert!(scroll.supports_action(Action::ScrollUp));
        assert!(scroll.supports_action(Action::ScrollDown));
        assert!(scroll.supports_action(Action::ScrollLeft));
        assert!(scroll.supports_action(Action::ScrollRight));
    }

    #[test]
    fn an_empty_semantic_tree_keeps_a_valid_window_root() {
        let update = full_tree_update("", [0, 0], f64::NAN, &AccessibilityState::default());
        assert_eq!(update.nodes.len(), 1);
        assert_eq!(update.focus, WINDOW_ROOT_ID);
        let root = converted_node(&update, 0);
        assert!(root.children().is_empty());
        assert_eq!(root.bounds(), Some(Rect::ZERO));
        assert_eq!(normalized_scale_factor(0.0), 1.0);
        assert_eq!(normalized_scale_factor(MAX_NATIVE_SCALE_FACTOR + 1.0), 1.0);
    }

    #[test]
    fn activation_reads_the_latest_thread_safe_snapshot() {
        let snapshot = Arc::new(Mutex::new(NativeTreeSnapshot::new(
            "初始",
            [10, 20],
            1.0,
            &AccessibilityState::default(),
        )));
        let first_snapshot = Arc::clone(&snapshot);
        let first = std::thread::spawn(move || {
            NativeActivationHandler::new(first_snapshot)
                .request_initial_tree()
                .unwrap()
        })
        .join()
        .unwrap();
        assert_eq!(converted_node(&first, 0).label(), Some("初始"));
        assert_eq!(
            converted_node(&first, 0).bounds(),
            Some(Rect::new(0.0, 0.0, 10.0, 20.0))
        );

        let state = state_with(semantic_node(
            1,
            "按钮",
            Data::Nil,
            BTreeMap::new(),
            &["点击"],
            [1.0, 2.0, 3.0, 4.0],
            Vec::new(),
        ));
        snapshot
            .lock_recover()
            .replace("更新", [30, 40], 2.0, &state);
        let second = NativeActivationHandler::new(snapshot)
            .request_initial_tree()
            .unwrap();
        assert_eq!(converted_node(&second, 0).label(), Some("更新"));
        assert_eq!(converted_node(&second, 0).children(), &[NodeId(1)]);
        assert_eq!(
            converted_node(&second, 1).bounds(),
            Some(Rect::new(2.0, 4.0, 8.0, 12.0))
        );
    }
}
