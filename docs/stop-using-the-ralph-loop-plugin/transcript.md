# Ralph Loop vs Ralph Wiggum Plugin - Video Transcript

## 00:00:00

Everybody's been going nuts about the Ralph Wigum plugin inside of Cloud Code, but there's one problem. It's not the real Ralph framework. But don't just take my word for it. Take a look at everything that the creator of the Ralph framework has been posting on Twitter and YouTube over the last few days. And his message is pretty clear. The original Ralph Loop framework is not the same as the Ralph Wiggum plugin that everybody's been talking about on YouTube, even though people make you think they're the same.

## 00:00:28

So, what exactly is the proper Ralph Loop technique? And why is the official anthropic Ralph Wiggum plugin falling short? Well, that's exactly what we're going to cover today. And I'm also going to show you how to implement proper Ralph loops inside of Cloud Code in the second half of the video. There's been a ton of misinformation swirling about around this topic. So, let's dive into it. So, what is Ralph? Now, this is from the blog of the creator himself. He writes

## 00:00:56

that Ralph is a technique and in its purest form, Ralph is a bash loop. Ralph, for all intents and purposes, distilled into its purest form is one line of code. It's just a while loop. It's just the ability to have an AI system attempt the same task over and over and over again until it gets it right. But the devil is in the details when it comes to implementing this looping system. And the way the original Ralph loop works and the way the Claude code plug-in works does differ in significant areas. So let's talk about

## 00:01:26

the Ralph loop at large from the original standpoint, the original Ralph loop. So how do we start this off? Well, we start with an idea, right? You show up to Claude Code and you have an idea for some sort of project, right? You want to create a canban board for social media content creators, right? Doesn't matter. You have an idea. That idea then becomes a product requirements document. You talk with Claude Code back and forth. You take your very mediocre prompt and you turn it into a document

## 00:01:51

that explains exactly what you're trying to build. It explains what the features are of that build and it takes those features and it breaks them down into discrete tasks, right? You know, I want to have the camb board. Well, you know what? It needs to have task one. It needs an edit button. Task two, it needs delete button. And task three, I want to be able to move it around or something, right? We're taking the big idea and we're distilling it into its smallest parts into tasks. Now is where the Ralph

## 00:02:18

loop comes in. So the Ralph loop, the way it's supposed to work is we've created this PRD. We execute the Ralph loop and importantly it starts a brand new session. What do I mean by that? I mean it's like actually ex exiting cloud code, spinning up a new cloud code instance. So we have a completely fresh context window. This is where these two diverge. This is where we have the difference between the original and the difference between the cla code version because the original starts a new

## 00:02:47

session. The cla code version doesn't. And this is very important. And this is important because the entire point of Ralph loops is that it hinges the on the idea that we are using a new session with a brand new context window because having a brand new context window means we're going to get way better outputs. And that's all because of the idea of context rot. Now, if you've watched my videos before, you know I've talked about this big picture. What is context rot? They've done studies on this on

## 00:03:12

multiple LLMs. It means the longer I talk with a large language model, the more I fill up its context window, the worse it gets. There's no specific drop off point, right? There's not an exact number, but you can kind of ballpark it is like once you get past the halfway point in terms of clawed code, that's 100,000 tokens. The effectiveness of the system will drop off dramatically. And again to sort of belver the point with the context window thing because I think it's important and this is the

## 00:03:36

difference between the real loops and the plug-in is like hey we have our context window right we have zero tokens up here 200k down here. Okay the first half we're smart right cloud code's crushing it. Opus 4.5 is doing everything we want to do. We want to stay in this area. This is good. This is thumbs up. We love being smart. Once we get past 100,000 tokens roughly we start becoming dumb, right? And we don't want to be dumb. This is bad. Okay. So to stay out of the dumb area, we start a

## 00:04:07

new session. So for every task, we start at zero. So obviously we want to stay in the smart area as long as possible because if we're in the dumb area, eventually we end up posting on Reddit complaining that Claude is quantitizing our models, right? So that's the reason that the new session idea is so important. So we execute the Ralph loop. What happens? Right? We actually want to do something now. So I start the Ralph loop. The route loop starts a new session of cloud code. It then takes a

## 00:04:32

look at the product requirements document. Says, hm, what am I supposed to build today? It then takes a look at the tasks that need to be completed. So, it's going to look what has or hasn't been done yet. And it's going to start on the first task that hasn't been completed. In our case, we haven't done any task. So, it's going to start on task one. It's going to code code code. If it completes the task, it's going to do a few things. One, it's going to update the PRD and say, "Hey, task one

## 00:04:56

complete." It's also going to update a second document, the progress.ext document. This is just a text document that goes into more detail of what it has completed each se each session. So, it's going to say, "Hey, you know, for task one, we did A, B, and C. This is what happened. Here's some patterns that emerged." Okay. Once it completes task one in that first iteration, we're going to execute the Ralph loop again autonomously. It's going to do that on its own, and it's going to start a new

## 00:05:21

session. So, what does it do again? It reads the PRD. It takes a look at task two again with a fresh context window. Codes codes tries to complete the task. Now, what happens if we can't complete that task? And this is really where the power of the route loop comes in. What happens if we don't finish this task, right? We almost run through a whole context window. Doesn't work. What happens? Well, again, what is it going to do? It's going to update the progress MD file, the text file, the progress

## 00:05:48

file, and say, "Hey, we tried to do task two. It didn't work. We tried A, B, and C. We got errors one, two, and three. And then it's just going to start the loop again, right? New session, iteration number two on task two. But this time, it's also going to be able to look at the progress file and say, "Hey, oh, we already tried A, B, and C, so let's try D, E, and F." Right? And it's going to repeat that process over and over. And by default, it will do that 10

## 00:06:14

times for each task until it completes it. Right? That's why you hear people saying, "Oh, it just repeats the task over and over and over and over again." Right? But the point of this isn't just that it repeats the task, that it repeats the task with the context from progress and repeats the task with a new session, right? So the additional context of previous iterations and the new context windows. What makes this so powerful? And so that's a Ralph loop. It just repeats that process over and over

## 00:06:39

and over again till it completes everything. Now, you're probably asking, how does that differ from the plug-in? Well, specifically, the Ralph Wigum plugin does not start a new session. Right? I'm looking at the Cloud Code GitHub right now. This is inside the Ralph Wickham plugin and you can see explicitly it says Cloud Code automatically works on the task, tries to exit, it then blocks the exit and then just continues until completion. So what are we doing? We're not getting a new context window.

## 00:07:11

We stay in the dumb section for longer until we hit the auto compact. Right? And auto compact, when does that start? That starts at what? 45 150,000 tokens, right? So, we're not getting a brand new session each time. It's just going to keep shoving more and more context into the window until it autoco compacts and starts again. So, every iteration, you aren't starting fresh, right? You're just starting wherever it happens to end. And so, in essence, we're losing one of the most powerful things of the

## 00:07:40

Ralph loop, which is the additional context tokens for each session. Right? The power isn't that we iterate this 10 times. The power isn't that we start over. The power isn't that we just bang our head against the wall forever because it's AI. No, no, that's not the power. The power is the new tokens. The power is context management. And if you watch previous videos like GSD, you know, the get done thing, it had a similar idea where it used sub aents. Okay, but if we don't if we don't

## 00:08:05

refresh the context window, we're defeating the what's the point? So, all that to say is this Cloud Code plugin that you're seeing everywhere, everybody's hyping this up like, oh my god, like it just repeats the thing forever and ever and ever. It's like, yeah. So, So we don't care that it does it 10 times. I wanted to do it 10 times effectively in a way that makes sense. So understand that those are the two biggest differences between these two systems and that this claud code system

## 00:08:32

does kind of like I think miss the point. So that's the Ralph loop and how it differs from Enthropic's version of it. Now let's talk about how to get the real quote unquote Ralph loop running in your own cloud code instance. Now there's a couple things we need to do. Now, first we need the actual script, and that's what we're looking at here. Now, like we talked about in the intro, the base version of this is very simple, right? It's essentially one line of code, and we're just going to add some

## 00:08:55

scaffolding on top of it. And I'm going to actually give you this file, you know, if you just want it. I'll put it inside the free school community. So, if you take a look at the pinned comments, there's the free school. Tons of free resources in there. The first link will be for the paid school. That's more for people who are trying to spin up like their own AI agency, but like everything you see here will be in the free one, right? Just head there and search for like whatever I title this video. Um,

## 00:09:19

you'll find it there. So, all this is doing is we run this file and it's essentially going to spin up an instance of Cloud Code that's going to execute the first task it sees that isn't completed inside of our PRD. The other thing you're going to need when you do this is a proper PRD file because the script is going to be looking for a PRD.MD. Now, this was the one I created. It was a cananban board for content creators. And I'll show you what it looks like when actually create it from start, but

## 00:09:49

I wanted to take you through like what a completed version looks like. So, you can see, right, it had in this case like 10 different tasks, so to speak. And you can see as it completed them, it actually had a red check mark. So, you can imagine at the beginning they were all blank. And so what it does, right, like we talked about before, it's going to read this thing. The first time it sees one of these boxes unchecked, that's the task it's going to go ahead and attack. So to create this exact sort

## 00:10:13

of PRD, you can create a cloud skill for it. And again, I'll put those files inside of the school. So to do this yourself, just open up a new folder. And then I want you to take those scripts and just dump it into your folder. So you can see right here, I just dumped in the Ralph script. Now note, if it's your first time doing this, you're on like Windows for example, make sure you're in developer mode. If you're in WSL or Linux or Mac, there's nothing else you really need to do. Now, we just want to

## 00:10:36

create our PRD. So, I have the skill. So, I'm just going to do / PRD. Again, skill will be in the community. You can also just copy paste whatever text is in there and just tell Cloud Code to create the skill for you. But, it's just sld and then whatever you're trying to build. So, I just wrote, I want to build a canban board for content creators. It's then going to ask me a series of questions to try to get more information out of it. Now, there's nothing really special about this. You don't have to do

## 00:10:57

my PRD skill. You can just do plan mode inside of Cloud Code if you want to. What is important is that you create a prd file. It has to be prd.mmd, right? That's what this script is looking for. And you have to make sure that it has explicit tasks to complete. So here's a clarifying question. What type of content workflow? Blah blah blah. I just fill these out. And once you answer the questions, it's then going to create the PRD for you. Now, if you use the skill, you will see it also creates the

## 00:11:21

progress.ext file. So again, if you don't use the skill, just make sure you create that on your own. It can be completely blank. And so here's the PRD. And like we said, what are we really looking for? We're looking for the specific user stories, aka tasks, and then the discrete things it needs to do underneath it, right? It needs to initialize the project, set up the database, blah blah blah. We've broken everything down to its like most minute detail. That's what we want. So, that's

## 00:11:45

all you need to do in cloud code. And for the next step, we just have to run the script. And then it's just going to do everything I explained in the first half of the video. Look at each task, try to complete it, spin up a new session. All hands off. Now, to do that, we're just going to open a new terminal. And when you're inside that new terminal, then you're just going to run the script. And right now, it's going to work on the first iteration. And you can pretty much just walk away at this point

## 00:12:08

cuz it's going to go through the whole PRD as many iterations as it needs to per task up to 10. And at the end, you'll be done. Now, as you run the script on this new terminal, this is what it will look like. It will say starting Ralph max 10 iterations. It defaults to 10. If I wanted more iterations after the script, right, after this Ralph script up here, I would have just done like space and then like 20, 30, however many iterations. But the iterations as if they go as planned, you'll see at the end, you'll say

## 00:12:34

there's still 44 incomplete tasks remaining. Iteration for 001 complete, 002 complete. There's more tasks remaining, right? It's just telling you that, hey, I did this. This is what's left. Your real source of truth in this case is going to be the PRD. I have that up right now. And you can see, hey, 001, which was the first task, it completed all the acceptance criteria, it's now moved on to two, and now it's going on to task three. Now, the whole iterations thing can be a little confusing. So,

## 00:13:02

let's say I had 20 tasks and I ran my script and I defaulted to 10 iterations. At most, it's going to complete those 10 tasks, right? That means every single task takes at least one iteration. And if it fails, right, it's going to add another iterate. it's going to attempt another iteration on that task. So, all that to say, if you have like 50 tasks and you really wanted to just like AFK this, like just walk away from your keyboard and come back when you run the script, just tell it that you also

## 00:13:30

wanted to do it like a 100 times. Again, it defaults to 10. You just add that number at the end of the script if you wanted to do more, but this is what it's going to look like. And now, I'll skip ahead and show you what it looks like when it's totally completed. So, here's the camb board it created. Again, this was very simple. This was essentially a oneshot, but it does everything I asked it to, right? We have three boards, ideas in progress done. I can easily drag and drop them. It updates the

## 00:13:53

counter on top when it does. So, I can add them. Right? The point of this video wasn't the project. The point was to show you the Ralph loop. But hey, it actually did what we wanted. And again, it was totally hands-off. So, that's the Ralph loop in a nutshell. And hopefully, you kind of understand the difference between its original implementation and the actual Cloud Code plugin at this point. Now, you're not incorrect using the Ralph Wigum plugin. Just kind of know some of the limitations going in,

## 00:14:17

especially when related to the context window. Now, a question I've gotten a lot since I posted the Get Done video the other day was, should I be using GSD or should I be using the Ralph loops? I don't think you can go wrong with either. They have the same sort of fundamentals when it comes to like breaking things down to discrete tasks and using fresh context windows. Personally, I kind of like GSD a little bit more. I think it does a little bit more handholding for the user. um especially when it comes to you getting

## 00:14:42

in there and testing it hands-on, but it's kind of personal preference. So, let me know in the comments what you thought. You can find all my resources again inside of my school community. There's a link to that in the comments. And as always, I'll see you
