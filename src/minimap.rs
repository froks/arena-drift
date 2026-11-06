//! Corner minimap: a dot for every entity with an [`Icon`].

use std::collections::HashMap;

use bevy::prelude::*;

use crate::CurrentArena;

/// Put this on anything that should show up on the minimap.
#[derive(Component, Clone, Copy)]
pub struct Icon {
    pub color: Color,
    /// Size in world units (scaled down to the map); dots never go below 3 px.
    pub size: Vec2,
    /// Higher layers draw on top (cars over obstacles).
    pub layer: i32,
}

#[derive(Component)]
pub struct MinimapRoot;

#[derive(Component)]
struct Dot;

const MAP_WIDTH: f32 = 230.0;

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, sync_minimap);
    }
}

fn map_size(arena: Vec2) -> Vec2 {
    Vec2::new(MAP_WIDTH, MAP_WIDTH * arena.y / arena.x)
}

/// Spawn the (empty) minimap frame; dots are added by `sync_minimap`.
pub fn spawn_minimap(commands: &mut Commands, arena: Vec2, marker: impl Bundle) {
    let size = map_size(arena);
    commands.spawn((
        marker,
        MinimapRoot,
        Node {
            position_type: PositionType::Absolute,
            right: px(12),
            bottom: px(12),
            width: px(size.x),
            height: px(size.y),
            border: UiRect::all(px(2)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.05, 0.05, 0.06, 0.85)),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.35)),
    ));
}

fn sync_minimap(
    mut commands: Commands,
    current: Res<CurrentArena>,
    root: Query<Entity, With<MinimapRoot>>,
    tracked: Query<(Entity, &Transform, &Icon)>,
    mut dots: Query<(&mut Node, &mut BackgroundColor), With<Dot>>,
    mut map: Local<HashMap<Entity, Entity>>,
    mut last_root: Local<Option<Entity>>,
) {
    let Ok(root) = root.single() else {
        map.clear();
        *last_root = None;
        return;
    };
    if *last_root != Some(root) {
        // New round: the old dots went away with the old frame.
        map.clear();
        *last_root = Some(root);
    }

    let arena = current.def().size;
    let size = map_size(arena) - Vec2::splat(4.0); // inside the border
    let scale = size.x / arena.x;

    for (entity, tf, icon) in &tracked {
        let dot_size = (icon.size * scale).max(Vec2::splat(3.0));
        let p = tf.translation.truncate();
        let left = (p.x / arena.x + 0.5) * size.x - dot_size.x / 2.0;
        let top = (0.5 - p.y / arena.y) * size.y - dot_size.y / 2.0;

        if let Some(&dot) = map.get(&entity)
            && let Ok((mut node, mut bg)) = dots.get_mut(dot)
        {
            // Only touch the node when something changed, to keep layout cheap.
            if node.left != px(left) || node.top != px(top) {
                node.left = px(left);
                node.top = px(top);
            }
            if bg.0 != icon.color {
                bg.0 = icon.color;
            }
            continue;
        }
        let dot = commands
            .spawn((
                Dot,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(left),
                    top: px(top),
                    width: px(dot_size.x),
                    height: px(dot_size.y),
                    ..default()
                },
                BackgroundColor(icon.color),
                ZIndex(icon.layer),
            ))
            .id();
        commands.entity(root).add_child(dot);
        map.insert(entity, dot);
    }

    // Drop dots whose entity is gone (pickups collected, barrels exploded).
    map.retain(|entity, dot| {
        let keep = tracked.contains(*entity);
        if !keep && let Ok(mut d) = commands.get_entity(*dot) {
            d.despawn();
        }
        keep
    });
}
