use std::{path::Path, sync::Mutex, time::Instant};

use bevy::{
    camera::visibility::NoFrustumCulling,
    dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin},
    input::mouse::{AccumulatedMouseMotion, MouseWheel},
    prelude::*,
    render::view::NoIndirectDrawing,
};
use instancing::{CustomMaterialPlugin, InstanceData, InstanceMaterialData};
use roto::Val;
use script_manager::ScriptManager;

mod instancing;
mod script_manager;

fn main() {
    let mut args = std::env::args();
    let path = args.nth(1).expect("need a path to a script!");

    App::new()
        .insert_resource(ScriptManager::new(Path::new(&path)))
        .add_plugins((
            DefaultPlugins,
            CustomMaterialPlugin,
            FpsOverlayPlugin {
                config: FpsOverlayConfig::default(),
            },
        ))
        .add_systems(Startup, setup)
        .add_systems(Startup, time_in_roto_setup)
        .add_systems(
            FixedUpdate,
            (
                reload_script,
                add_particles,
                update_particles,
                update_instances,
                time_in_roto_update,
            ),
        )
        .add_systems(Update, orbit)
        .run();
}

static EMITTER: Mutex<Vec<Particle>> = Mutex::new(Vec::new());

#[derive(Component)]
struct Particles(Vec<ParticleWithTime>);

struct ParticleWithTime {
    start_time: f32,
    particle: Particle,
}

#[derive(Clone, Debug)]
struct Particle {
    pos: Vec3,
    scale: f32,
    color: Color,
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(0.5))),
        InstanceMaterialData(Vec::new()),
        NoFrustumCulling,
    ));

    commands.spawn(Particles(Vec::new()));

    commands.spawn((
        Camera {
            clear_color: ClearColorConfig::Custom(Color::from(Srgba::rgb(0.0, 0.0, 0.05))),
            ..default()
        },
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
        NoIndirectDrawing,
    ));
}

fn reload_script(mut manager: ResMut<ScriptManager>) {
    manager.reload();
}

fn add_particles(
    time: Res<Time>,
    mut manager: ResMut<ScriptManager>,
    mut particles: Single<&mut Particles>,
) {
    if let Some(add) = &manager.add {
        let t1 = Instant::now();
        add.call(&mut (), time.elapsed_secs());
        let t2 = Instant::now();
        let duration = t2 - t1;
        manager.add_ms = (duration.as_secs_f64() * 1000.0) as f32;
    } else {
        // let mut rng = rand::rng();
        // let x = rng.random_range(-10.0..10.0);
        // let y = rng.random_range(-10.0..10.0);

        // let particle = Particle {
        //     pos: Vec3 { x, y, z: 0. },
        //     scale: 1.0,
        //     color: Color::from(Srgba::RED),
        // };
        // EMITTER.lock().unwrap().push(particle);
    }

    let mut e = EMITTER.lock().unwrap();
    for particle in e.drain(..) {
        particles.0.push(ParticleWithTime {
            start_time: time.elapsed_secs(),
            particle: particle.clone(),
        });
    }
}

fn update_particles(
    mut manager: ResMut<ScriptManager>,
    time: Res<Time>,
    mut particles: Single<&mut Particles>,
) {
    let Some(update) = &manager.update else {
        return;
    };

    let t1 = Instant::now();
    particles.0.retain_mut(|p| {
        let t = time.elapsed_secs() - p.start_time;
        let particle = p.particle.clone();
        let res = update.call(&mut (), t, Val(particle));

        if let Some(Val(new)) = res {
            p.particle = new;
            true
        } else {
            false
        }
    });
    let t2 = Instant::now();
    let duration = t2 - t1;
    manager.update_ms = (duration.as_secs_f64() * 1000.0) as f32;
}

fn update_instances(
    particles: Single<&Particles>,
    mut instances: Single<&mut InstanceMaterialData>,
) {
    instances
        .0
        .resize(particles.0.len(), InstanceData::default());

    for (p, i) in particles.0.iter().zip(&mut instances.0) {
        i.position = p.particle.pos;
        i.scale = p.particle.scale;
        i.color = LinearRgba::from(p.particle.color).to_f32_array();
    }
}

#[derive(Component, Clone, Copy)]
enum TimeInRotoText {
    Add,
    Update,
    Particles,
}

fn time_in_roto_setup(mut commands: Commands) {
    commands
        .spawn((
            Node {
                // We need to make sure the overlay doesn't affect the position of other UI nodes
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                right: px(0),
                align_items: AlignItems::End,
                ..Default::default()
            },
            // Render overlay on top of everything
            GlobalZIndex(i32::MAX - 32),
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("Add: "),
                TextColor(Color::from(Srgba::WHITE)),
                TimeInRotoText::Add,
                Pickable::IGNORE,
            ))
            .with_child(TextSpan::default())
            .with_child(TextSpan::new("ms"));
            p.spawn((
                Text::new("Update: "),
                TextColor(Color::from(Srgba::WHITE)),
                TimeInRotoText::Update,
                Pickable::IGNORE,
            ))
            .with_child(TextSpan::default())
            .with_child(TextSpan::new("ms"));
            p.spawn((
                Text::new("Particles: "),
                TextColor(Color::from(Srgba::WHITE)),
                TimeInRotoText::Particles,
                Pickable::IGNORE,
            ))
            .with_child(TextSpan::default());
        });
}

fn time_in_roto_update(
    query: Query<(Entity, &TimeInRotoText)>,
    mut writer: TextUiWriter,
    manager: Res<ScriptManager>,
    particles: Single<&Particles>,
) {
    for (entity, time_in_roto) in &query {
        match time_in_roto {
            TimeInRotoText::Add => {
                *writer.text(entity, 1) = format!("{:>6.2}", manager.add_ms);
            }
            TimeInRotoText::Update => {
                *writer.text(entity, 1) = format!("{:>6.2}", manager.update_ms);
            }
            TimeInRotoText::Particles => {
                *writer.text(entity, 1) = format!("{:>8}", particles.0.len());
            }
        }
    }
}

#[derive(Debug, Resource)]
struct CameraSettings {
    pub pitch_speed: f32,
    // Clamp pitch to this range
    pub pitch_range: std::ops::Range<f32>,
    pub roll_speed: f32,
    pub yaw_speed: f32,
}

fn orbit(
    mut camera: Single<&mut Transform, With<Camera>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut mouse_wheel_reader: MessageReader<MouseWheel>,
    time: Res<Time>,
) {
    let pitch_limit = std::f32::consts::FRAC_PI_2 - 0.01;
    let camera_settings = CameraSettings {
        pitch_speed: 0.003,
        pitch_range: -pitch_limit..pitch_limit,
        roll_speed: 1.0,
        yaw_speed: 0.004,
    };

    let delta = mouse_motion.delta;
    let mut delta_roll = 0.0;
    let mut delta_pitch = 0.0;
    let mut delta_yaw = 0.0;

    if mouse_buttons.pressed(MouseButton::Left) {
        // Mouse motion is one of the few inputs that should not be multiplied by delta time,
        // as we are already receiving the full movement since the last frame was rendered. Multiplying
        // by delta time here would make the movement slower that it should be.
        delta_pitch = -delta.y * camera_settings.pitch_speed;
        delta_yaw = -delta.x * camera_settings.yaw_speed;
    }

    // Conversely, we DO need to factor in delta time for mouse button inputs.
    delta_roll *= camera_settings.roll_speed * time.delta_secs();

    // Obtain the existing pitch, yaw, and roll values from the transform.
    let (yaw, pitch, roll) = camera.rotation.to_euler(EulerRot::YXZ);

    // Establish the new yaw and pitch, preventing the pitch value from exceeding our limits.
    let pitch = (pitch + delta_pitch).clamp(
        camera_settings.pitch_range.start,
        camera_settings.pitch_range.end,
    );
    let roll = roll + delta_roll;
    let yaw = yaw + delta_yaw;
    camera.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);

    // Adjust the translation to maintain the correct orientation toward the orbit target.
    // In our example it's a static target, but this could easily be customized.
    let target = Vec3::ZERO;

    let mut distance = camera.translation.length();
    for mouse_wheel in mouse_wheel_reader.read() {
        distance -= mouse_wheel.y * 0.1;
    }
    camera.translation = target - camera.forward() * distance;
}
