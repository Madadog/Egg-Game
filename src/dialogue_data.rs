// Copyright (c) 2023 Adam Godwin <evilspamalt/at/gmail.com>
//
// This file is part of Egg Game - https://github.com/Madadog/Egg-Game/
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program. If not, see <https://www.gnu.org/licenses/>.

use crate::{
    dialogue::TextContent::{self, *},
    portraits, sound,
};

// Strings directly printed with `print_raw()` must end with a
// null byte `\0`, while strings printed by the game's dialogue
// system must not.
pub const GAME_TITLE: &str = "super unfinished EGG GAME\0";
pub const GAME_TITLE_BLURB: &str = "version 0.0.15\0";
pub const MENU_PLAY: &str = "Play\0";
pub const MENU_OPTIONS: &str = "Options\0";
pub const MENU_BACK: &str = "Back\0";
pub const MENU_EXIT: &str = "Exit to Menu\0";
pub const MENU_DEBUG_CONTROLS: &[&str] = &[
    "Palette 1\0",
    "Palette 2\0",
    "Palette 3\0",
    "Remove CameraBounds\0",
    "Toggle Dog\0",
    "Add creature\0",
];
pub const OPTIONS_TITLE: &str = "super unfinished OPTIONS MENU\0";
pub const OPTIONS_FONT_SIZE: &str = "Toggle Font Size\0";
pub const OPTIONS_RESET: &str = "Erase Data\0";
pub const OPTIONS_RESET_SURE: &str = "Erase Data (Press again to confirm)\0";
pub const OPTIONS_LOSE_DATA: &str = "You'll lose all data.\0";
pub const BEDROOM_MATTRESS: &str = "You can't get to sleep.";
pub const BEDROOM_TROLLEY: &str = "It's your baby bro's cot.";
pub const BEDROOM_CLOSET: &str = "Everything you have is in here.";
pub const BEDROOM_WINDOW: &[TextContent] = &[
    Text("It's a beautiful day...\n\n"),
    Delayed("... Outside.", 30),
];
pub const THING: &[TextContent] = &[
    Text("This thing is..."),
    Pause,
    Portrait(Some(&portraits::Y_NORMAL.to())),
    AutoText("This thing is absolutely terrifying! Why the heck did I interact with it?!?"),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Because you are a mere shell occupied by the player, possessing no real control over your actions."),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_AWAY.to())),
    AutoText("Pff... That sounds like something a bad writer would say... What are you supposed to be, anyway?"),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("I am the gamedev, and I have come to give the player some useful debug instructions for this unfinished game."),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("Uhh... YOU'RE the gamedev?"),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Yes."),
    Delayed("\n...", 30),
    Delayed("\n... What's with that look?", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("... WHY"),
    Delayed(" are you an indescribable eldritch monster-thing?", 20),
    Delayed(" ... Like, you could have picked literally any form.", 40),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("This IS literally any form."),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_OOF.to())),
    AutoText("I meant literally any OTHER form!"),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("What's wrong with this body in particular?"),
    Delayed(" ... I happen to like it.", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_CLOSE.to())),
    AutoText("It's,"),
    Delayed(" like,", 30),
    Delayed(" really offputting,", 30),
    Delayed(" I guess?!", 30),
    Portrait(Some(&portraits::Y_OOF.to())),
    Delayed(" I dunno, I just really hate staring at it!!!", 30),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Art is fundamentally about self-expression."),
    Delayed(" Your opinions are magically invalid.", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_OOF.to())),
    AutoText("Oof....."),
    Delayed(" You asked for my input,", 30),
    Delayed(" while intending to ignore it from the very beginning?!?", 30),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Yep."),
    Delayed(" On the topic of your input...", 30),
    Delayed(" as you specifically requested, you can pet the dog now.", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_NORMAL.to())),
    AutoText("Awesome!"),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("...."),
    Delayed(" Hardest", 30),
    Delayed(" feature", 30),
    Delayed(" yet........", 30),
    Delayed(" :(", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("Uhhh..."),
    Delayed(" You had trouble with...", 20),
    Delayed(" Petting???", 20),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Yes."),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("..."),
    Delayed(" Really?", 30),
    Delayed(" ... That's...", 20),
    Delayed(" I don't think it could've been that hard to implement...", 20),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("I mean, reaching over and petting the dog..."),
    Delayed("\n...", 30),
    Delayed(" It's like, two steps at most?", 30),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("You fail to grasp just how bad my codebase REALLY IS..."),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("..."),
    Delayed("\n...", 30),
    Delayed("\n... How bad can it BE, really?", 30),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Well, consider the dog's hitbox."),
    Pause,
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("... Even now, only the drawing code really knows where each party member is at any time..."),
    Delayed(" Everything else just sort of guesses.", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_REGRET.to())),
    AutoText("I shouldn't have asked..."),
    Pause,
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("Wait..."),
    Delayed("... Is this why I'm still stuck inside an empty map devoid of content or living creatures even after literal weeks of development?", 30),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("No, that's just poor planning..."),
    Delayed(" As for \"no living things\"...", 30),
    Pause,
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("What about that creature on the couch? He seems like quite the fellow if you just got to know him"),
    Flip(false),
    Portrait(Some(&portraits::Y_AWAY.to())),
    AutoText("... Wow."),
    Delayed(" That joke gets funnier every time you reuse it.", 30),
    Delayed("\n...", 30),
    Delayed("\n... What's his deal, anyway?", 30),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Why don't you ask HIM that?"),
    Delayed(" ... I mean, I don't believe you've exchanged even a single word with him...", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_OOF.to())),
    AutoText("Because the game doesn't let me talk to him!"),
    Delayed(" It's", 30),
    Delayed(" like", 30),
    Delayed(" he's literally an inanimate object!!!", 30),
    Delayed(" I'm not ignoring him by choice!", 30),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("So you're saying that it's somehow my fault that you've neglected that poor creature?"),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("Uhh, yes?"),
    Delayed(" You're the one who didn't give him any dialogue...", 30),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("On the topic of moral and ethical responsibility,"),
    Delayed(" do you want some cheat codes?", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_NORMAL.to())),
    AutoText("HECK yeah, how do I access the real game?"),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("... This IS the real game..."),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_AWAY.to())),
    AutoText("lame......"),
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Press [a] to access the debug menu. Hold [ctrl+shift] to noclip. [n] to see memory usage. [m] to see map stuff. [d] for player info."),
    Pause,
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("The number keys also do some stuff. If you don't have a keyboard.... Tough luck, dude!"),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_NO.to())),
    AutoText("Hold on, I didn't write any of that down!!!"),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Sorry, I only say it once. You're just gonna have to replay this entire conversation to see the shortcuts again!"),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_AWAY.to())),
    AutoText("... Surely you could just play a shorter version of this dialogue the second time? You already do that with the stairwell text, right?"),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("Haha, you'd think so, but I can only do conditional branches for single-line text!"),
    Delayed("\nHooray for code!", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("Just how bad at programming ARE you???"),
    Pause,
    Flip(true),
    Portrait(Some(&portraits::HORROR.to())),
    AutoText("I prefer to think of myself as a"),
    Delayed(" \"chef\"...", 30),
    Delayed("\n... In a restaurant...", 30),
    Delayed("\n... Making spaghetti...", 30),
    Pause,
    Flip(false),
    Portrait(Some(&portraits::Y_REGRET.to())),
    AutoText("I wish I was in a different game..."),
];
pub const HOUSE_STAIRWELL_WINDOW: &str = "The glimmering gold sun ignites the hills, casting wild shadows over the landscape. You feel hopeful.";
pub const HOUSE_STAIRWELL_WINDOW2: &str = "By a twisted error of design, the view here lines up precisely with the neighbours' bathroom window.";
pub const HOUSE_STAIRWELL_PAINTING_INIT: &str = "It's not as good as the real thing.";
pub const HOUSE_STAIRWELL_PAINTING_AFTER: &str = "A yellow circle hovers listlessly over a collection of purple lumps. You feel nothing in particular.";
pub const HOUSE_STAIRWELL_DOOR: &str = "You shouldn't go in there.";
pub const HOUSE_LIVING_ROOM_COUCH: &str = "He's busy.";
pub const HOUSE_LIVING_ROOM_TV_1: &str =
    "It's a cartoon of some sort. The protagonist is still charging up his main attack.";
pub const HOUSE_LIVING_ROOM_TV_2: &str = "Some sort of cartoon. While the hero was charging his attack, the villain took over the world.";
pub const HOUSE_LIVING_ROOM_TV_3: &str = "A long cartoon series. After taking over the world, the villain created global stability and happiness.";
pub const HOUSE_LIVING_ROOM_TV_4: &str = "Still the same cartoon. The hero finished his training arc, beat the villain and became the new ruler.";
pub const HOUSE_LIVING_ROOM_TV_5: &str = "This cartoon series refuses to end. The hero made some bad choices, now everyone wants the villain back.";
pub const HOUSE_LIVING_ROOM_TV_6: &str = "This series will last forever. The hero murdered the villain out of spite. His only enemy is the world.";
pub const HOUSE_LIVING_ROOM_WINDOW: &[TextContent] = &[
    Text("You harbour some very strong feelings about gothic windows.\n"),
    Delayed("None of them are good.", 30),
];
pub const HOUSE_KITCHEN_CUPBOARD: &str = "The cupboard is empty. Even the spiders have moved on.";
pub const HOUSE_KITCHEN_SINK: &[TextContent] = &[
    Sound(&sound::ALERT_UP),
    Delayed("Found something down the drain...!\n", 0),
    Text("... You left it there."),
];
pub const HOUSE_KITCHEN_WINDOW: &str =
    "The unholy king of tacky windows. Words fail to convey your antipathy.";
pub const HOUSE_KITCHEN_MICROWAVE: &str = "Microwave, the oven of the future. It cooks everything; bread, mince meat, oxygen absorption packets...";
pub const UNKNOWN_1: &str = "The wispy ethers of your moral fibre hold this door shut.";
pub const UNKNOWN_2: &str = "... Quick, recast the \"Sleep\" shell on your opponent to keep it perma-stunned! Unfair and effective!";
pub const UNKNOWN_3: &str = "... Don't get your hopes up.";
pub const DEFAULT: &str = "You kids don't know what it's like. In my day, the gamedev didn't even assign me any interaction dialogue.";
pub const DOG_OBTAINED: &str = "Dog has joined the party!";
pub const DOG_RELINQUISHED: &str = "Dog has left the party.";
pub const HOUSE_BACKYARD_BASEMENT: &str = "A horrendous stench rises from the cellar.";
pub const HOUSE_BACKYARD_SHED: &str = "The shed door won't budge, but you could easily open it with some of the POWER TOOLS inside... Oh wait.";
pub const HOUSE_BACKYARD_SHED_WINDOW: &str =
    "You can't actually see anything through this window.";
pub const HOUSE_BACKYARD_NEIGHBOURS: &[&str] = &[
    "You don't know much about the neighbours.",
    "... The traffic makes it difficult to get to their house.",
];
pub const HOUSE_BACKYARD_DOGHOUSE: &str = "SUBROUTINE \"DOG\" NOT FOUND. INITIATE DEFAULT SUBROUTINE: knock knock. whos there. no response. laughter.";
pub const HOUSE_BACKYARD_STORMDRAIN: &str = "Over the fence lies a deep canal. There is no way back up, not unless you can return from the dead.";
pub const HOUSE_BACKYARD_ANTHILL: &[&str] = &[
    "bb_grin",
    "Hey sis, let's play \"stick our fingers in the anthill!\"",
    "y_disgust",
    "... Wow, uh,,,,,,",
    "y_judgy",
    "... Maybe, let's not????",
    "bb_joy",
    "Whoever keeps their finger the longest wins~~!!!",
    "y_joy",
    "You're on~!!!",
    "y_sober",
    "Ok, but seriously, how long have you been playing?",
    "bb_grin",
    "I've been here *all* morning...... Heh, it kinda stings, actually!",
    "y_worry",
    "... You're bleeding... and we don't have any more PlasterAids...",
    "bb_grin",
    "I'll be right as rain, don't you sweat, sister... Juuuust as soon as I get all these ants off me...",
    "y_worry",
    "... That is a LOT of ants...",
    "bb_worry",
    "They're in my socks!",
    "y_horror",
    "I'll get the hose.",
    "bb_horror",
    "Please hurry!!!",
];
pub const TOWN_TRAFFIC: &str = "They've been stuck like this for a while now.";
pub const TOWN_LAMPPOST: &str =
    "Strangely enough, this pole isn't casting a shadow. This will undoubtedly become relevant later.";
pub const TOWN_HOME_WINDOW: &str = "It's not as bad from the outside.";
pub const TOWN_WIDE: &[TextContent] = &[
    Text("T"),
    Delayed("h", 10),
    Delayed("i", 10),
    Delayed("s", 10),
    Delayed(" ", 10),
    Delayed("t", 10),
    Delayed("e", 10),
    Delayed("x", 10),
    Delayed("t", 10),
    Delayed(" ", 10),
    Delayed("i", 10),
    Delayed("s", 10),
    Delayed(" ", 10),
    Delayed("w", 10),
    Delayed("i", 10),
    Delayed("d", 10),
    Delayed("e", 10),
    Delayed("...", 10),
];
pub const INVENTORY_TITLE: &str = "INVENTORY";
pub const INVENTORY_ITEMS: &str = "Items";
pub const INVENTORY_SHELL: &str = "Shell";
pub const INVENTORY_OPTIONS: &str = "Options";
pub const INVENTORY_BACK: &str = "Back";
pub const ITEM_FF_NAME: &str = "French Gry";
pub const ITEM_FF_DESC: &str =
    "A fried, thinly-sliced potato snack shaped like a guy. You feel a little bit ill.";
pub const ITEM_CHEGG_NAME: &str = "Mystery Egg";
pub const ITEM_CHEGG_DESC: &str = "There's something inside.";
pub const ITEM_LM_NAME: &str = "Little Man";
pub const ITEM_LM_DESC: &str = "\"... Hello? Somebody there? ... It's awful dark in here...\"";
pub const SM_COIN_RETURN: &str = "There's no money in the coin return slot.";
pub const SM_FRUIT_BASKET: &str = "They're not fresh.";
pub const SM_MAIN_WINDOW: &str = "It's yellow outside.";
pub const SM_FRIDGE_1: &str =
    "If you blow on the glass, it fogs up, revealing all the fingerprints left by prior employees.";
pub const SM_FRIDGE_2: &str =
    "A note on the front says \"Out of Order\", followed by a smiley face. A sickening contrast.";
pub const SM_VENDING_MACHINE: &str =
    "The blurb reads \"It's SodaTime!\". The machine is filled to the brim with cans of motor oil.";
pub const CONSTRUCTION_1: &str =
    "Looks like the creator didn't put too much effort into this part of the map.";
pub const CONSTRUCTION_2: &str = "Looks like it's still under construction.";
pub const EMERGENCY_EXIT: &str =
    "This is an emergency exit. It's not an emergency right now. Ergo, you cannot use the exit.";
pub const SM_HALL_SHELF: &str = "There's a single bottle of floor cleaner. And no mop.";
pub const SM_HALL_WINDOW: &str = "Looks like this window has recently been painted yellow.";
pub const EGG_1: &str = "It's floating.";
pub const SM_STOREROOM_SHELF: &str = "They're all out of Keratin Krunch.";
pub const SM_TITLE: &str = "S____MAR__T";
pub const INSTRUCTIONS_TITLE: &str = "Instructions";
pub const INSTRUCTIONS: &str = "Arrow keys: Move around.\n\n[Z]: Interact.\n\n[X]: Open inventory, Skip text.\n\n\nRemember to get regular sleep.\n\n\n\n    Press any button to continue.\0";
// pub const _: &str = "";
