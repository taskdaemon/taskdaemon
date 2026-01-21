# Recursive Language Models (RLMs) Paper Discussion Transcript

So, this paper has been sitting in my head since I read it because the implications are a bit mad if it holds up. It's a scalable approach to extending the effective context window of your AI agents. And we're not talking just by a bit here. We're talking by one or two orders of magnitude beyond the context window. And that is significant. You should pay attention to this because it unlocks opportunities that previously agents and language models struggled with. We're talking about due diligence across hundreds of documents, potentially local models running your AI agents on your code bases, extending this to very large code bases now, millions of lines of code. The most exciting thing about this is the building blocks for this exist today. This paper just rubber stamps it. So, let me walk you through it.

All right. So, this is a really interesting paper on recursive language models released by MIT really recently. I think this was dropped at the end of last year. It's actually, funny enough, I'm embarrassed. It's one that I missed, but we're going to go through it today together. I think there's a lot of interesting implications for large language model context use and how that scales to perform more and more complex tasks over a longer context.

So, let's let's have a read of the the argument um the abstract. So we study allowing large language models to process arbitrarily long prompts through the lens of inference time scaling. We propose recursive language models, RLMs, a general inference strategy that treats long prompts as part of an external environment and allows the LLM to programmatically examine, decompose, and recursively call itself over snippets of the prompt.

All right. So, we find that RLMs successfully handle inputs up to two orders of magnitude beyond model context windows and even for shorter prompts, dramatically outperforming the quality of base LLMs on common long context scaffolds across four diverse long context tasks while having comparable or cheaper cost per query.

So, that's really interesting. First of all, the first thing that stands out to me is this like two orders of magnitude improvement or extended capability of uh standard model context windows. So what they've said is the RLM successfully handle inputs up to two orders of magnitude beyond the model context window. So if you have a million token context, I think one order of magnitude would be 10 million, two orders is 100 million. That's amazing. If you can get the model to work across 100 million tokens, not necessarily performance improvements. I'm probably jumping ahead here, but I guess that's what they're trying to say with this approach.

So, that's interesting. So, recursive language models and it recursively calls the LLM to process that document rather than stuffing it into the entire context window. And what's surprising here as well is it's comparably cheaper. uh maybe not surprising if it's just processing what it needs, but it's comparably cheaper as in cost per query than stuffing the entire thing into the context window. And it is also better than common long context scaffolds.

So this is kind of really interesting for me because um right now if anyone that uses claude code knows that claude code deals quite well with a code base which it can be thousands tens of thousands of lines of code millions of tokens well 10 million tokens easily could be a code base right and Claude's code can successfully process that so that's an interesting thing here I want to see how this approach differs to the approach that claude code is currently using.

## Figure 1 Analysis

So in the introduction here they've got this interesting chart and the chart is showing the performance of GBT5 just raw versus GPT5 using this RLM recursive language model scaffold and you can see the degradation. Now we're going to get into a bit more of what these tasks actually are. Um, but treat the top task as kind of like a baseline task, a benchmark. And then you've got Ulong and Ulong pairs. And you can see the GBT4 raw like as you extend that context, the performance just drops off a cliff.

And this is again presenting this argument that just because your model can handle 1 million token context, it doesn't mean it's effective over 1 million token context. And you know, this chart on the left with GPT5 shows that. And then interestingly, this RLM approach with scaffolding GPT5 with this recursive language model technique, you maintain performance higher over that context window. And they've even tested into the above 262k tokens to a million. And it doesn't seem to be much performance degradation between 262,000 tokens and a million, which is amazing.

So yeah, there it's really interesting approach has a lot of implications for technical deep technical work. Imagine if you're a lawyer reviewing contracts and stuff that can be really dense and technically heavy or policy documents and you need to get that right. So this has a lot of interesting implications for those fields and processing such heavy context. I know a lot of people have been using claude code to try and do that already. But let's see what this does above and beyond.

## Context Rot and Limitations

Is there any useful information here in this figure one? What is it saying? It's just describing the performance. So we've already talked about that. Okay, there's something here about the limitations. So it says despite rapid progress in reasoning and tool use, modern language models still have limited context lengths, which we know even within these limits appear to inevitably exhibit context rot.

Okay, so that's describing the diagram above. Context rot is a familiar problem for all of us. You've probably felt it when you've used clawed code or you've used chatbt and the conversation gets too long or the task gets too long and it starts to kind of deteriorate. They've used autoco compact as a earlier versions of claw code you'd just have to restart the session and the early days of cursor as well if you're doing it for coding you'd have to restart the session and start again but now they use this autoco compat feature.

So the phenomenon illustrated in the left hand side of figure one where the quality of the frontier models like GPT5 degrades quickly as context gets longer right so we know that that's just context rot and I think this is basically just saying you get better performance with the RLMs.

## Inference Time Compute

So I won't I won't go into all of this and let's move on to the next paragraph. We studied the question through the lens of scaling inference time compute. Anyone got any thoughts on inference time compute as a label? I don't want to digress too much, but it seems to me that we were talking about test time compute before with reasoning. Why is this called inference time compute? Some of the labels in Genai really do confuse me, but anyway, we move on.

We draw broad inspiration from outofcore algorithms in which the data processing systems with a small but fast main memory can process far larger data sets by cleverly managing how data is fetched into memory. That's interesting. So giving it the kind of scaffold gives it that ability to manage a data set that's much larger than the context window. So when they say small but fast memory probably they're talking about the context window here. So that's the general pattern. And the LLM has a small but fast memory can manage something much bigger than itself and also has the kind of intelligence behind it to manage something much bigger than itself.

## Context Compaction Limitations

All right. So inference time methods for dealing with what are in essence long context problems are very common though typically task specific. One general and increasingly popular inference time approach in this space is context condensation or compaction. So that's what I was talking about earlier with claw code. That's what we do and they've mentioned open AI here and a few they've talked about this in which the context is repeatedly summarized once it exceeds a certain length threshold.

Unfortunately, compaction is rarely expressive enough for the task that require dense access to many parts of the prompt as it presumes in effect that some details that appear early in the prompts can safely be forgotten to make room for the new content. Yeah, and that's a big problem. Everyone that's kind of used any type of autoco compaction or summarization the biggest issue with summarization is obviously it's lossy. It's a lossy compression. So what do you keep and what do you discard sometime that's task dependent and I think some people have tried to create intelligent compaction where it has an understanding of what the task was in the first place to decide what to keep but yet it seems like a very brittle method and you know this paper is highlighting that as well.

## The RLM Approach

So we introduced recursive language models, a general purpose inference paradigm for dramatically scaling the effective input and output lengths of modern LLMs. The key insight is that long prompts should not be fed into the neural network the transformer directly but should instead be treated as part of the environment that the LLM can symbolically interact with. So that's what we're talking about the program programmatically interacting with the long context data rather than trying to like stuff everything into the LLM memory.

So this is kind of going against the intuition that we need to extend context windows. If we can take that context window and make it effectively work over two orders of magnitude greater than what it is, then maybe we don't need to extend context windows beyond a million tokens. Or maybe we top out at 10 million for usefulness. Who knows, right?

As figure 2 illustrates, an RLM exposes the same external interface as an LLM. It accepts a string prompt of arbitrary structure and produces a string response. So this reads to me like it's just a scaffolding. Let's get into more details.

## How RLMs Work

Given a prompt P, the RLM initializes a read eval print readal print loop programming environment in which P is set the value of the variable. It then offers the large language model general context about the ripple environment. So the length of the string P and permits it to write code that peaks into and decomposes P and to iteratively observe any side effects from execution.

Crucially, RLMs encourage the LLM in the code it produces to programmatically construct subtasks on which they can invoke themselves recursively. So it constructs sub task to invoke the LLM again to kind of look into this task.

So I guess a good abstraction for recursion here is you get the LLM to first of all process this this object P which is if you're talking in the domain of law or due diligence it might be a data room full of 100 documents or it might be a legal contract that is 100 pages long full of dense information. So the RLM programmatically breaks that down and constrains and narrows and narrows it by recursion.

So it will look and say okay section two is important. Section 2 clause 5 is important. Uh section 2 clause 5 relates to section 3 clause 2. That's important. So it narrows and narrows the task down programmatically. And that's what the recursion allows it to do. The recursion allows it to focus on ever more sub specialized bits of that long document that require attention at that point. That's that's the the recursion factor.

By treating the prompt as an object in an internal environment. This simple design of RLMs tackles a foundational limitation in many prior approaches. So they are referencing anthropic here which focuses on recursive decomposition of tasks but cannot allow their input to scale beyond the context window of underlying LLM. So I wonder what that means cuz isn't this ripple approach still recursive decomposition? And what is the blocker to not allowing the input to scale beyond the context window of the LLM? If anyone knows the answer to that, just drop it in the comments, but that'll be interesting to understand.

## Models Tested

We evaluate RLMs using a Frontier closed model, GPT5, and a Frontier open model, which is the Quen Coda model. It's a 480 billion parameter model. These days, that is probably classed as a mediumsiz model. I remember back in the day when back in the day was just maybe 2 years ago but when a 480 billion parameter model was scary now you can stand up a 1 trillion parameter model on um hardware available to a consumer.

So that is that's interesting. If this problem really does solve the context window problem, we might see the first like real open-source agentic products, you know, where everything runs on your laptop rather than having to call cloud services. Anyway, I digress.

Let's keep reading. across four diverse tasks with varying levels of complexity for deep research, information aggregation, code repository understanding and synthetic pair-wise reasoning tasks where even frontier models fail catastrophically. That will be interesting to examine the reports. And by the way, all of these tasks you've got software engineering covered there. You've got legal analysis, policy, anything to do with long document extraction symphysis is covered there. So this is an approach that has a lot of downstream impacts on real GDP generating use cases.

## RAG Comparison

Let's say we compare ILM against the direct LLM calls and as well as the context compaction retrieval tool use agents. I realized I skipped the diagram. So let's go back to the diagram so we can understand how it works.

So we have the language model. The language model produces that prompt and this is what you'd call a prompt object. This is the environment. The environment here, just so no one's confused, is just a Python interpreter. That's what it looks like. It's simply Python interpreter. That's the place where the language model can submit Python code to be executed. And because it can be executed, you can observe the result. So that's part of that ripple loop that we're talking about, the scaffolding.

And then you've got your prompt which exists in the environment as loading in as a variable. And then the language model is iterating over that prompt recursively to try and break down what's in there and move it on its way to achieving the goal that has been set.

So here's what's interesting, right? There's been this big push for rag like over the last few years and rag has kind of if you're on the frontier of research, rag has kind of fallen out of favor and that's because it's it's quite a brittle approach. The way rag works is rag was one of the early attempts to overcome context window limitations. In the early days of like GBT 3.5, the context windows were really small. We're talking about you lucky if you get like 8k tokens context, 8,000 token context, which is basically an essay.

Context windows were really small and large language models were frozen back then. They weren't connected to the internet anything. And if you ask them any any question, it would be wrong. it would be based on their training data. So ask them like who's the president of the United States right now? It wouldn't know the answer to it. It would just know based on its training data.

So people use rag as a way to overcome this. The brittleleness with rag is in the retrieval. The retrieval itself is very rudimentary. It's matching on semantic similarity. The next most rudimentary retrieval mechanism is going to be your semantic similarity. Logical relationships necessarily. It's just based on are these things semantically similar.

## Why RAG Breaks

So that breaks and this is a great example of it. So if you read the prompt here, look what they're trying to do, right? Best to read it here cuz it gives you the whole thing. So it says you are reading an extremely long book. Can you list out all of the items that were made before the great catastrophe? Right? And then it's given the book or whatever. Right?

So this is the prompt and the variable is loaded into the prompt as well. So the book loaded into the prompt and you can see the language model is already trying to dissect it and assess what parts are important to focus on in that book. Rag would break here because rag will try to do a semantic similarity. So what you would end up retrieving with rag if you're doing the traditional semantic retrieval is you'd get the sentences that are semantically similar to the prompt.

That in this case is not necessarily going to return you accurate responses, especially as the context grows. You need something more intelligent. You need something more adaptable. This adapts based on its own context that it's retrieved and it sets its own sub goals to better narrow down what that is.

So you can see what it's doing here. This is the input. You've got your input. It's printed the prompt out and then it's delivered an output and the language model acts on that output again and finally has managed to triangulate that answer. So yeah, it's a very powerful approach.

## Scaling to Long Context Tasks

Let's go down. So SC scaling to long context task. This is an interesting section. I just want to make sure we haven't skipped anything. Apologies I'm jumping around a little bit but bear with me. Let's see what's what this is about.

So scaling to long context task. Recent work has successfully argued that effective context window of LLMs can often be much shorter than the model's physical maximum number of tokens. So we know this. Going further, we hypothesize that the effective context window of an LLM cannot be understood independently of the specific task.

So that's an interesting intuition and that is more complex problems will exhibit degradation at even shorter lengths than simpler ones. It's trying to get us to think about these things in tandem, right? Like it's not just about having a long context window. If you have a long context window and you provide a problem that requires crunching through a lot of tokens and it's a really difficult problem, you might only get 1% 10% use of that context before the model just fails completely.

And maybe that's why we're seeing poor performance in a lot of these kind of legal chat bots and things that people have been complaining about saying they hallucinate because legal work or kind of due diligence work or anything like that requires such heavy think about the policies or contracts or technical documentation that sorry policies and contracts. It's dense and very heavy in terms of logic in there and statements and things like that. This is it's really complex and that's probably why traditional approaches have been failing. U because of this.

## Task Characterization

We characterize tasks in terms of how their complexity scales with prompt length. Interesting. So they're characterizing task in terms of how their complexity scales with the prompt length. For example, needle in a hasty problems generally keep needles as constant as prompt length is scaled. As a result, while previous generations of models struggled with near tasks, front tier models can reliably solve these tasks in ruler, even in 1 million plus token settings.

Nonetheless, the same models struggle at shorter lengths on oolong, which is a task where the answer depends explicitly on almost every line in the prompt. So, this is going back to the diagram in the opening page of this. Let's just shift up. So this is the NIA task. And what it's effectively saying is this is a solved problem. As the context grows, the needles in the haststack don't grow. It's just picking up the same needles.

And I think if you know what needles you're picking up, probably rag can solve this problem. Like traditional semantic retrieval maybe can solve this needle in a haststack problem cuz it's going to do a similarity match on rag. But I think it's even just saying raw models without rag alone can solve this needle in a haststack problem too. And you can see the evidence up here.

## Logical Coherence Challenge

But these ulong problems where there is some kind of there's logical coherence between different parts of the document which means rag is not going to solve your problem. So the I'm breaking it down simply to say logical coherence means one paragraph or one sentence depends on what was written previously and so on. Right? So everything is kind of a logical mapping.

The models struggle more with things like that and you can scale that. Right? you can scale to the extent to which there is logical coherence or a logical relationship between each of the points.

I'll give you an example. So if you have something like a Q&A document, right? Maybe you're serving an organization serving customer insurance policy or whatever and there's a Q&A document around that insurance policy. You can theoretically have a situation where every question and answer is independent of each other in that Q&A document. It's literally just a list of questions and answers that are completely independent of each other.

In that case, language model isn't going to to struggle. You can use rag on that very easily. Semantic retrieval match based on the similarity and pull back the right questions. And you don't have to worry that you've missed something because they're they're completely independent of each other.

But yeah, if those things are connected in any way, then performance starts to break down. And you know, a lot of real world tasks are that we want to unlock beyond this kind of independence. We're talking about legal contracts, policy documents, code bases, all of these things where there's dependency. One item cannot just be looked in isolation. It depends on something that is mentioned somewhere else in the document.

So yeah you what they're saying is you can scale that level of internal coherence.

## Benchmarks

Let's continue reading. So it says grounded in this intuition we design our empirical evaluation around tasks where we are able to verify not just the lengths of the prompts but also cons consider different scaling patterns for problem complexity. Got it? So yeah it's the intuition I was trying to communicate earlier. They're scaling the problem complexity along with the length of the prompt.

We loosely characterize each task by information density. I.e. how much information an agent is required to to answer the task and how this scales with different size inputs. So if if you're in that like world where you've got a thousand Q&A questions all independent of each other, you really probably only need one of those question answer pairs to answer that task. But if you're in a world where you've got a complex legal document with clauses that reference other clauses and all of these types of things, you might need to read the whole document to understand the task. you might need to read 75% of it. For some sections, you might need to read 30% of it. Um, depending on the task to understand the document. So, I think that's the intuition here.

### SNI (Single Needle in Haystack)

So, S9 are following the single needle in a hast task in ruler. We consider a set of 50 single needle in a haststack task that require finding a specific phrase or number in a large set of unrelated text. These tasks require finding a single answer regardless of input size. and as a result scale roughly constant in processing costs with respect to input length.

And is that because you're using rack because I I would imagine if you I get this kind of 50 single needle in a haststack task right so what they're doing is you've got 50 needles in a hstack and they're scaling from I don't know zero tokens all the way to a million tokens to see how the model performs across that right but you still only have 50 needles and so the challenge that for me it says it's rough constant in terms of processing costs. But I would have thought if you're just stuffing that raw into an LLM, I would have thought that would increase your processing cost because you're using more tokens as you move from zero tokens to a million tokens unless you're using rag. So if you're using rag, potentially I could see this being true.

### Browse Comp Plus

So, we've got another benchmark or test here, which is browse comp plus, which is 1,000 documents. A multihop question answering benchmark for deep research questions that require reasoning over multiple different documents. The benchmark provides a verified offline corpus of 100,000 documents that is guaranteed to contain gold, evidence, and hard negative documents for each task.

Following sunet hour, we use 150 randomly sampled tasks as our evaluation set. We provide 1,000 randomly chosen documents to the model or in which the gold and evidence documents are guaranteed to exist. We report a percentage of correct answers.

The answer to each task requires piecing together information from several documents, making these tasks more complicated than SNI despite also requiring a constant number of documents to answer. So this seems to me like this is modeling something like if you're a software engineer and you're reading documentation, sometimes you need to understand how this API you're pulling data from this API and how it interacts with something over there in another document reference.

So this is kind of useful for those I would imagine this approach or this is modeling a situation like that. It's multihop question answering across large corpus of documents. So notice here this is limited because you know the corpus itself is 100k documents. So we're not going over 100k documents. It's over 150 randomly sampled tasks over a thousand randomly chosen documents. Those documents obviously curated well. So you can answer the task from the documents. That's what it's saying. But even a thousand documents, a lot of the real use cases that I have come across are like hundreds of documents. So a thousand performance on a thousand documents tells us enough. But yeah, this is multihop retrieval across multiple documents, thousand scale documents.

### ULong

All right. Ulong a long re ulong. So a long reasoning benchmark that requires examining transforming chunks of the input semantically. examining and transforming chunks of the input semantically then aggregating these chunks to form a final answer.

So we report scoring based on original paper which um scores numeric answers as a score of y equals.7 and other answers as exact match. So what's what is this saying? I I don't I don't know quite what this is measuring. score as a is y of.7 to the power of y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y y hat, right? So I have no idea what that's measuring, but maybe it become clear as I read and other answers as exact match.

We focus specifically on the trek course split, which is a set of 50 tasks over a data set of questions with semantic labels. Each task requires using nearly all entries of the data set and therefore scales linearly in processing cost relative to the input length. So I'm not quite sure I understand that a long reasoning benchmark that requires examining transforming chunks of input semantically then aggregating these chunks to form a final answer.

Okay. So this is this is all about synthesis. That's what it's talking about here. So without getting into the mathematical detail of things you know without getting too confuses this is measuring the performance of synthesis that's how I'm reading it.

### ULong Pairs

Then we've got ulong pairs. So when I say synthesis is like research and synthesis. It's like can you identify the right information and again can you put it together in a form that we want you to put it together in um oolong.

So ulong pairs we manually modify the tree trek core split of ulong to include 20 new queries that specifically require aggregating pairs of chunks to construct the final answer. In appendix one we explicitly provide all queries in this benchmark. We report F1 scores over the answer F1 record precision.

This is embarrassing. I always forget these um data science metrics off the top of my head. But anyway, that's just a method for examining recall versus precision, I believe. Yeah, let's let's talk about this again.

So, we manually modified the trees split of Uong to include 20 new queries that specifically require aggregating pairs of chunks to construct the final answer. So, specifically require aggregating pairs of chunks to construct the final answer. So, it's just it's a specific type of synthesis.

I'm trying to think of a practical application for this. pairs of chunks to extract the so maybe it's like question answer over some policy documents basically right like you you have pairs of chunks or maybe it's like identifying contradictions or something like maybe identifying contradictions over a vast pool of legal documents right so that's or a vast pool of any type of documents I don't know but that's what it seems like to me right.

## Precision and Recall Explained

So in appendix E1 we explicitly provide all queries in this benchmark so we can look at some of the queries we report F1 scores over the answer. So here F1 is balancing precision and recall. So if you have let's just go into what those things are.

So precision is how often you are correct or how often the model is correct or the AI is correct when it predicts a when it says gives us an answer. So, if you've got an AI predicting looking at somebody and predicting if they are if they going to if they're going to be sick or not, let's say based on some symptoms and it's like a binary decision, they're either sick or they're not. Every time the model predicts the patient to be sick and it's actually correct, that is more precise. 90% precision would be the model predicts 10 sick people and nine out of 10 were correct.

Whereas recall is how you can recall all of the sick patients in a population. So let's say a model is looking at a population of 100 people and there are definitely 10 sick people in that population and the model only gets one person in that population. I think the recall then is just 10%. Whereas if it gets 10 people in that population then the recall is 100%. But notice you can have very high recall with low precision. So the model could predict 50 people sick and the recall would still be 100%. But that's obviously much lower precision because only 10 out of that 50 were actually correctly sick.

All right, so that's precision and recall. And then F1, a higher F1 means higher precision and recall. A lower F1 means one of those things has dropped. So F1's a good thing to track because it's no use being precise and having low recall. Like, yeah, you predicted one sick patient, but actually we need you to get 10, and it's no good having high recall and low precision. Sometimes it is, but you want high recall and high precision.

So, we report F1 scores over the answer. Each task requires using nearly all pairs of entries of the data set and therefore scales drastically in processing costs relative to the input length. Interesting. So as the input length grows then the compute time for this scales to the power of two basically. So that's interesting.

### LongBench v2 Code QA

Longbench v2 code QA a multi-choice code repository understanding split from longbench v2 that is challenging for modern frontier models. Report the score as the percentage of correct answers. Okay. Each task requires reasoning over a fixed number of files in a codebase to find the right answer. Right. So that's the applications for that are obvious.

## Experimental Setup

We compare RLMs against other commonly used task agnostic methods for for each of the following methods. We use two contemporary large LMS GPT5 one reasoning. Oh, we've already talked about this. So they're just using GBT5 and the Quen Coder 480B using sampling parameters described in team 2025 chosen to provide results for commercial and open frontier model respectively. So for Quen Kodo 3 we compute costs based on the fireworks provider.

In addition to evaluating the base model on all tasks we also evaluate the following methods and baselines. Okay. So RLM with the ripple approach we implement an RLM that loads its context as a string. Okay. So this is just is this just repeating the ripple stuff. This is just talking about the details of the implementation. Look if you want to like I I'm not going to go into the details of the implementation here but it's just giving us uh RLM with ripple no sub calls.

All right let's have a look. So the system prompt is fixed across all experiments. For GPT5 experiments we use GT5 mini. for the recursive LMS and GBT5 for the root LM as we found this choice to strike a powerful trade-off between the capabilities of RLMs and the cost of the recursive course.

So the root the one that's kind of like orchestrating at the top is GBT5 and then the recursive models where we get more specialized and narrow is GT5 mini and that is that's like a pragmatic tradeoff for like a production use case for this. So that's that's an interesting setup. It'll be interesting to see what system prompt they had.

### RLM with No Sub Calls

All right. So, RLM with no sub calls. We provide an ablation of our method in it. The ripple environment loads in the context but is not able to use sub LLM calls. So, sublm calls. In this setting, the LM can still interact with its context in the ripple environment before providing a final answer.

So, this is the full approach with no recursion. No, that doesn't make sense. So, yeah, just sense check with chatb. I think I was just confusing myself, but I had it on track, right? So, what it is is one has the ability to do that recursion and the other doesn't have the ability to do that recursion. So, from a kind of higher level abstraction, you can already see that one will be less flexible than the other. And therefore I would expect that the RLM with ripple and no sub calls where you remove that recursive ability is going to be less effective.

But yeah so they've got three approaches. is I got the RLM with ripple rm with ripple no sub call so removing the recursion I believe and then the summary agent following sun at we consider iterative agent that invokes a summary of the context as it's filled for example a given corpus of documents it will iteratively view the documents and summarize when full in cases where provided context exceeds the model window the agent will chunk the input to fit within context text window and invoke the same strategy over these chunks.

For GBT5, due to the extremely high cost of handling large token inputs, we use GBT5 nano for compaction and GBT5 to provide the final answer.

All right, so yeah, all they're doing is they're summarizing this is stuff that we see happening in in Claude code.

### Code Act + BM25

And then there's another method code act plus BM25 BM25. I think that is like keyword match almost or is it like it's hybrid retrieval, isn't it? I forgot.

We compared directly to codeact agent that can execute code inside of a react loop. So react is reason and act loop. That was a thing that we put in place before we had true reasoning model. So it's kind of the most basic react loop that I remember was like searching something on the internet observing it and then adjusting your plan to then go out and find something.

I was trying to use React Loops for multihop question answering when I built Jared which was a a experimental chatbot that I built maybe a few years ago to test multihop question answering with large language models. Feels like a long time ago now.

Unlike RLM, it does not offload its prompts to the code environment and instead provides it directly to the LM. Furthermore, following Himalayas and Chen Adal, we equip the agent with a BM25 retriever that indexes the input context for tasks where this is appropriate. So this is rag. This BM25 retrieval BM25 combines traditional keyword search with modern semantic search dense embeders. Yeah, that's what we're going for. So BM25 is just your your combination of getting keyword and semantic search. So this is kind of testing a rag based approach basically.

## Results and Discussion

Results and discussion. We focus our main experiments in table one on the benchmark described in 2.1. Furthermore, we explore how Frontier model and RLM performance degrades as input context grow. All right, so let's have a look.

So I wonder is there a is there a chart plotting this? It might be easier to see on a chart size described in table 2.2. I wish they didn't do this. I wish it was just easy to see. Okay, so that's code QA. What task is this?

So take a performance comparison of different methods across long context benchmark. So varying complexity in gray is the average API cost plus or minus the standard deviation of okay integrates where the method ran into input context limits. Okay. Okay. And that obviously only happens on the ones that are not the RLMs.

I'm just trying to process what this data say. Ah right. Okay. You've got code QA at the top. Prowcom Ulong Ulong pairs. Got it. And this is a task length range. So, CoQa is the shortest. Oh, no. Sorry. CoQa is ranging from 23K to 4.2 million. So, these are the the token lengths of the tasks.

What's interesting is the Ulong and Ulong pairs aren't that long, but they are scaling the complexity of the document. They've got that complexity score. I wish is there anywhere where they're reporting that complexity score as well? cuz that would be interesting to see.

## Key Findings

But and what are we measuring here? So performance comparison. Ah yeah obviously all of these have their own benchmark that they're scoring against but this is a performance comparison.

I think the gist of it is that RLM with no sub calls outperforms on code QA and browse comp with the Quen 3 coder model. That's interesting. Um, and with GBT5, the RLM outperforms everywhere.

So that's interesting because, you know, GPT5 is by benchmarks is a a much more capable model than the Quen 3 coder 480p. So I wonder if the recursion hurts you if you're using a double model. So you have to have a really intelligent model to orchestrate that recursion. And that that obviously makes sense.

And yeah, look, there's a there's a big difference in these scores here. Like there's 91 on this browser comp versus the quen coder 480B. And yeah, I it's hard to it's hard to know the pattern within the model itself. So within the model performance itself, whether the RLM with no sub calls is how often this would be the case across how many what range of tasks this would be the case that it would perform better.

But what this tells me is it's unstable. you need to reach a certain level of intelligence of the model before that stabilizes and where your sub calls actually make a difference and I think you see that here with GBT5. So that's interesting that seems like I don't know like an emerging behavior really. So that's that's a really interesting observation from this.

## Observation 1: RLM Scales to 10M+ Tokens

All right so let's check out the observations cuz this where it gets meaty and this where it gets interesting. So RLM can scale to 10 million plus token regime and can outperform base LMS and existing task agnostic agent scaffolds on long context tasks.

Across all tasks, RLMs demonstrate strong performance on input tasks well beyond the effective context window of Frontier LM, outperforming base models and common long context scaffolds by two times the performance while maintaining comparable or cheaper average token cost.

Notably, RLM scale well to the theoretical cost of extending a base models context window on browse comp plus 1K. The cost of GBT 5 minute mini ingesting 6 to 11 million input tokens is $1 $1.50 to $2.75 while RLM GBT5 has an average cost of 99 and outperforms the summarization and retrieval baselines by over 29.

So it's always I think they're suggesting in this experiment on their sample it was always cheaper to go with the RLMs and the performance were better. So you get that performance uplift and the efficiency after this.

Furthermore, on task where processing costs scale with the input context, RLMs make significant improvements over the base model on tasks that fit well within the models context window.

On Ulong, the RLM with a GT5 and Quen 3 coder outperformed the base model by 28.4% and 33% 33.3% respectively.

On ulong pairs, both cheap5 and quen freod make little progress with F1 scores of less than 0.1% while RLM using these models achieve F1 scores of 58% and 23% respectively, highlighting the emerging capability of RLM to handle extremely information dense tasks.

Wow, that is that's massive. That's a big gap. That's like completely useless to Now we're getting somewhere. And you know, I I'd read a comment on this and somebody mentioned that this 5 model has not even been specially trained to use this scaffold either. So that is that's fascinating. It seems like there's probably scale to achieve here. We could probably continue to improve this observation or there's not scale but improvements to achieve here. We're at 58% just off like this experimental approach.

## Observation 2: REPL Environment is Key

Observation two, the ripple environment is necessary for handling long inputs while the recursive sub calling of RLMs provides strong benefits on information dense inputs. So yeah, this is where the legal context and all of that stuff comes in like when you need that information density because it allows you to break down the relevant sections.

So a key characteristic of RLM is offloading the context as a variable in an environment epsilon which is your Python interpreter that the model can interact with even without suballing capabilities.

Our ablation of the RLM is able to scale beyond the context limit of the model and outperform the base model and other task agnostic baselines on most long context settings.

So on code QA and browse comp task this ablation is able to outperform RLM by 17.9% and 3% respectively. This is just saying that the RLM is better than the base on the ablation even.

So as long as you have the Python environment, the ripple environment, it allows you to handle long inputs and the recursive nature is what allows you to handle dense inputs. So depending on what you need, there's probably an engineering trade-off here. Basically say when do you need recursion? You need recursion when you've got information density. If you've got just length with no density, then you will need ripple.

So on information dense tasks like ulong or long pairs, we observe several cases where recursive LM sub calling is necessary. We see RLM perform the necessary semantic transformation line by line through recursive subcalling while the ablation without the sub calls is forced to use keyword heristics to solve these tasks across all information dense task. RLMs outperform the ablation without sub calling by 10 to 59%.

And I wonder how many layers you can go with this sub calling stuff like you know the main agent is able to call some smaller less intelligent agents. But when we get to a level where the sub agents are as fast as GBT5 mini with the intelligence of GBT5 then you've got another layer of sub calls that you can make. There's another layer of recursion you can make. And I don't know what type of complexity of tasks that would be able to solve. adding that third layer of sub calling um adding that additional layer of recursion.

## Cost Analysis

Cost of the RLM and baseline described in 2.2 plotted okay so this is the cost base RLM no sub calls so this is percentile cost so cost of RLM and baseline described in 2.2 to plotted at the 25th, 50th and 70th percentile of total API cost.

Okay, so this is the distribution of tasks Kodak summary agent. Wow. So yeah, we're just picking up the distribution of costs here across and yeah, the 95th percentile for this base model is the lowest. So that's the lowest cost distribution, especially for the raw model. If you're just chucking context in there naively, yeah, it's going to be cheaper, but it's not going to perform well.

Paying that little bit of extra cost for the RLM approach buys you a lot more in terms of return. And you see this especially the case with um GBT5, the more intelligent model versus Quen Free Coder 480B. uh that's actually only at the 95th percentile. Like at the median for the most part, the RLM approaches are more cost effective than the raw model.

So yeah, the data positioning this is a no-brainer really. It's just more cost effective all the way through. And even when it gets ugly, it doesn't get as ugly.

What is maybe surprising but not awfully surprising when you think about what it's doing is if you look at the summary agent the autoco compact and the code act agents where you have that BM25 retrieval the 95th percentile tasks are way higher and this is probably why you know used to cost a fortune in tokens when you use cursor back in the day who knows if they were using recursion at that point but it was costing a fortune in tokens so yeah you can see token efficiency here is greatly improved.

There is what I would say as well is there is a difference in the subcourts when you look at the weaker Quen model versus the GBT5 model. So sub calls versus no sub call approach when you do the RLM with no sub calls versus RLM with but I don't know how much to read into that.

The most striking observation really is that this chart is shown is it's just cheaper throughout and you get that uplift in performance and that uplift cost efficiency as well.

## Observation 3: Scaling Behavior

Observation three, LM performance degrades as a function of input length and problem complexity while RLM performance scales better. So yeah, this is amazing with the scaling.

The benchmark sni ulong and ulong pairs contain a fixed number of tasks over a context with length ranging 2 to the 13 to 2 to the 18. Furthermore, each benchmark can be loosely categorized by different processing costs of input context with respect to length roughly constant linear quadratic respectively.

In figure one, we directly compare an RLM using GBT5 to base GPT5 on each task. And we find that GPT5 performance degrades significantly faster for more complex tasks while RLM performance degrades but at a much slower rate which aligns with the Goldman at our 2025.

For context lengths beyond 2 to 14, RLM consistently outperforms GT5. So what are we saying here? So this is back in figure one. So this is just what we saw initially. The RLM maintains its performance over long context better.

So what about the complexity scaling? So RLM cost scale proportionally to the complexity of the task while still remaining in the same order of magnitude of cost of GBT 5 in. Okay. So as tasks get more complex, the cost does scale. We explore what choices the RLM makes in these settings that cause these differences in cost.

Lastly, in this setting, we also observe that the LM outperforms RLM in a small input context regime. Oh, that's interesting. So, if the context is small and it's there's small input, it's better to just go with the DLM. So, not a one size all fits approach by any means. So, it's not throw RLM at every problem. It's thronomat long context high complexity problems.

So is this for long context high complexity problems versus and short context high complexity problems or is it just short context in general? So we explore what choices RLM makes in settings that cause these cost differences.

Lastly in this setting we also observe that the base LM outperforms RLM in the small input context regime. By construction, an RLM has strictly more representation capacity than an LM. The choice of environment that calls the root LM is equivalent to the base LM. In practice, however, we observed that RLM performance is high is slightly worse on smaller input lengths, suggesting a trade-off point between when to use a base LM and when to use an RLM.

## Observation 4: Cost Variance

Observation four, the inference cost of RLMs remain comparable to base model call but are high variance due to the different trajectories length differences in trajectory length. So RMS iteratively interact with their context until they find a suitable answer leading to large differences in iteration length depending on task complexity and I bet across runs as well for the same task like they they might even take different routes.

In figure three we plot the quartile costs. So this is the table that we were just analyzing before. It's basically just showing that they are cheaper. They remain in the same order of magnitude with the raw model, but they're cheaper and they're definitely cheaper than the summary and compacting modes.

## Model-Specific Behaviors

RLMs are a model agnostic inference strategy, but different models exhibit different overall decisions on context management and sub calling. Yes. So this is what we picked up about like Quenfree coder when you bring in the when you bring in the the quen when you bring in the sub calling it actually suffers in performance versus without the sub calling or with the RLM.

And I guess it depends on the the capability and the type of model that you're using. I said it's more intelligent. If it's a more intelligent model it does better. But I don't know that that's just comparing GT5 with Quen Quen Koda 48B. um you might have a smaller model that could outperform GPT5 on this for a specialized task. So there's a few dimensions. There's model and there is task as well.

And what do they say? So they say while GBT5 and quener both exhibit strong performance on RLMs relative to their base model and other baselines, they also exhibit different performance and behavior across all tasks. Yeah. So it's task dependent.

On browse comp plus in particular. RLM nearly solves all tasks while RLM with GBT5 nearly solves all tasks while RLM with code free coder struggles to solve half.

We know that the RLM system prompt is fixed for each model across all experiments and is not tuned for any particular benchmark. Right? So it's the same. They've kept the prompts the same. So they're saying if they maybe if they adjusted the prompts they could get Quinn to perform better between GBT5 and Quen Freeod.

The only difference in the prompt is an extra line in the RLM Quen Free Coda prompt warning against using too many subcourse. Okay, but that seems like because the model isn't or maybe it's I don't know. Maybe it's biased to call sub routine after subutine. Who knows? I don't know. But that seems like a model capability thing to me.

We provide an explicit example of this difference in example B3 where RLM performs a semantic transformation in ulong as a separate sublm core per line while GPT5 is conservative about subquerying LLMs. Yeah, that seems like a model capability to me.

## Emerging Patterns in RLM Trajectories

Emerging patterns in RLM trajectories. Now this is interesting. So there's definitely a few wow moments in here and in the emerging patterns for RLM trajectories.

I think we're getting the sense that this architecture gives agents a lot more flexibility to be economical about how they process data basically and to follow more complex reasoning paths for processing that data. It's like handing off compute to sub agents and things like that. It's kind of like how claw code works already. CL code has sub agents and also access to a ripple environment too. I guess it's interesting from an academic standpoint, but I feel like industry has got here already.

Let me know in the comments if you feel the same. I can't see anything different from code, but anyway.

### Filtering Input Information

It says filtering input information using code execution based on model prior. The key intuition for why the RLM abstraction can maintain strong performance on huge inputs without exploding costs is the LM's ability to filter input context without explicitly seeing it.

That is interesting because I kind of wonder that as well, right? Like you just give it the long context and it can filter and pick up patterns and know how to dice things up without seeing it. That is interesting. It's like reasoning over the object in a way that you kind of do. Like we don't we don't necessarily need to read every word in a document like um you know we can do a control F and jump to one part and jump to another part.

I did an experiment just watching Claude code reason over a legal document using playright and it did it managed to find things in the document. We even track the trace. So yeah, that is interesting in and of itself like the ability to reason over context that it doesn't necessarily haven't explicitly seen all of it.

Furthermore, model prior enabled the RLM to narrow the search space and process fewer tokens. As an example, in figure 4A, we observed RLM GPT5 using regex queries search for chunks containing keywords in original prompt in the original prompt festival and phrases it had prior about leun across most trajectories.

Okay, the point is it can reason over the context that it hasn't seen all of it. So you don't need to stuff the entire context into the model.

### Chunking and Recursive Sub-calling

Chunking and recursively subcalling LMS. RLMs defer essentially unbounded length reasoning chains to sub RLM's calls. Unbounded length reasoning change. The choice of decomposition can greatly affect task performance especially for information dense problems.

In our experiments, we did not observe complicated partitioning strategies beyond uniform chunking or keyword searches. In figure 4B, RLM chunks by new line in a thousand plus long context from ULO.

I'm trying to see if I can get a handle on what is meant by that. So this chunking and recursively sub calling RLMs defer essentially unbounded length reasoning chains to sub RLM calls. Choice of decomposition can greatly affect task performance especially for information dense problems.

In our experiments, we did not observe complicated partitioning strategies beyond uniform chunking or keyword searches. RLMs defer essentially unbounded length reasoning chains to sub RLMs.

So I I think what this is basically saying is that this is what they're doing when they're trying to process this long context is pretty simple. It's like keyword keyword matching. What did they say they did? So now what did they observe can affect choice of decomposition gradient effects task performance. So strategies beyond uniform. Yes.

So they observed uniform chunking keyword searches right pretty simple things. Uniform chunking is just split code into chunks or you know match on keywords. But because of the combined with the recursion, combined with the sub calling, it creates this complexity, this flexibility that emerges from very simple primitives.

So you've got you've got something very simple as being able to segment your code by line or being able to match on a legal document by keyword. That leads to this greater power of recursion. Combine that with recursion and all of a sudden you've got something that can process really complex documents and process them to a to a good standard or you know process a huge code base and understand that huge codebase.

So yeah that's combining the simplicity with the architecture enables emergent behavior. That's an interesting thing here.

### Answer Verification

Answer verification through subLM calls with small context. So this is really interesting because I've been working on like factchecking and you know claim checking.

This is interesting because it's saying that we observe several instances of answer verification made by RLM through subLM calls. That's interesting. So some of these strategies implicitly avoid context rock by using subLM to perform verification while others solely use code execution to programmatically verify answers are correct.

In some instances, however, the answer verification is redundant and significantly increases the cost per task. In example, we observe a trajectory on Ulong where the model tries to reproduce its correct answer more than five times before choosing the incorrect answer in the end.

Let's have a look at this stuff. What are we looking at? Is this the scaffold end to end? Let's just have a look. So I think what they're saying is they observed instances of LLMs verifying themselves like answer verification and some of these strategies implicitly avoid context rot by using sublms to perform verification while others solely use code execution to programmatically verify answers.

In some instances however the answer verification is redundant and significantly increases the cost per task.

### Variable Passing for Long Outputs

Passive recursive LM outputs through variables for long output tasks. So RLMs are able to produce essentially unbounded tokens well beyond the limit of the base. Yeah, because they keep recursively calling each other. I see. So that is where the capacity comes in. The capacity comes in from the recursion because it can continue to call models further and further down the chain.

I wonder how they get it to do they like artificially block at a certain depth of the tree from the node. I wonder if they put that in place.

So RLMs are able to produce essentially unbounded tokens well beyond the limit of the base LM by returning variables as output and then yes they can process over that output right so they don't need to absorb the whole thing into context they can just like arbitrarily break it up and understand what's in there almost blind right like it's pretty pretty cool at the start anyway through the ripple the RLM can iteratively construct these variables as a mixture of programmatic and sub RLM output calls.

We observe the strategy used heavily in ulong pairs trajectories and that's like the one that's information and complexity dense where the RLM stored the output of sub LLM calls or subLM calls over the input variables and stitched them back together to form the final answer. So it's evidence of like complex decomposition and symphysis over a it's yeah decomposition and synthesis over a um long context fascinating.

## Limitations and Future Work

Related works I won't go into so there are some related works there long context LM systems limitations and future works so let's have a look at the limitations.

While RLMs show strong performance on task beyond the context window limitations of existing LMS at reasonable inference cost. The optimal mechanism for implementing RLMs remains underexplored. H interesting.

We focused on synchronous sub calls inside a Python ripple environment. But we note that alternative strategies involving asynchronous sub calls and sandbox RPLs or ripple as I'm calling them can potentially significantly reduce the runtime and inference cost of RLMs.

So when this is already fascinating because I think what I hadn't quite appreciated at the start of this is that they ran this whole experiment on synchronous recursion. So the root LLM or the master language model at the top of this RLM framework is calling a sub aent but it can't call sub aents asynchronously. It can't like wait for this has to wait for that sub agent to finish running and then interpret the results.

So that does actually limit the flexibility of the system but you can do this asynchronously. So you can have the root agent call multiple sub aents at the same time. Imagine the type of flexibility you would get there. So they saying they haven't explored that approach.

And I think what else are they saying? So we focus on synchronous sub calls inside a Python ripple environment. We note that alternative strategies involve asynchronous sub calls and sandbox ripples can potentially significantly reduce the runtime and inference cost of RLMs.

Wow. And I that makes you wonder as well like how does that stack up with smaller models like can you because you can call asynchronously are you able to send out more smaller dumb models? Um like you know are you able to send out a bunch of 7 billion parameter models go and kind of perform that recursion? I guess like when the 7 billion parameter models are calling subm models they they might fall over but I digress.

### Recursion Depth

Furthermore you choose to use a max regioning depth. All right. So, I was asking of that. So, it can only go down to a recursion of one and that still showed performance, but it might be one of these things like when we were doing like random forest models back in the day and you, you know, you set the the number of leaf nodes or the depth of the tree too far on certain tasks, you could overfit, but that might not be a problem for what you want to do here.

Like um if you're trying to automate something rather than trying to predict a you know trying to fit a model to do another process that it hasn't seen if you're trying to automate like an established process that might not be such a problem. Yeah.

So they set a max recursion depth so that's like tree depth of one. While we found strong performance on existing long context benchmarks we believe that future work should investigate deeper layers of recursion. That's amazing. That is amazing. We can see where this is going.

### Training for RLMs

Lastly, we focused our experiments on evaluating RLMs using existing frontier models. Explicitly training models to be used as RLMs could provide additional performance improvements.

And I think I mentioned that at the beginning, but yeah, they haven't even explicitly chained GBT5 or Quen to do these types of recursion tasks. So imagine now collecting all of that data and I'm sure there's already people implementing this in clawed code with sub agents. There are already people talking about this and there's been people talking about this kind of reminds me of agent swarms.

If you take the the asynchronous way of doing things that seems more like an agent swarm to me. So people have already been doing this. So imagine all that data that the frontier labs are able to collect while people are doing this with claw code and codeex and so on. I don't know if you can do sub agents in codeex but point still stands.

## Conclusion

All right. So what's the conclusion here? we introduce uh RLM's. Yeah. Look, so I think this is a a mindblowing abstraction. I think it's a very important abstraction for any engineers because it should change the way you solve certain problems.

I can already think of for example in the you know brain cube fact checker instead of just stuffing an entire long context document into the claim decomposition thing and providing a very complex prompt to decompose claims. You can easily apply an RLM to that. Easily treat the document as a data object, introduce a ripple environment and then get the RLM to perform the recursion over that to decompose into claims and that might be cheaper and it might also be better performing.

So yeah, this is exciting for me personally for you know that project but it's just exciting for anyone building engineering AI agents like this is really the way you want to go for those tasks are like legal tasks or complex documents really complex documents things that are information dense but also things that are long 200page legal contracts merger agreements things along those lines that we have struggled to get large language models to perform over because you know they can perform well over a paragraph of complex information or you know something like a math question they might be able to reason over but a 200page merger agreement is like has so far failed.

So yeah this has amazing implications. This is a great paper and you know if you like this style of content where I just riff and show you how my brain is thinking about things you know while I read these papers and process tasks please do comment. I can make more of it.

In fact it's probably a bit easier to make more of this style of content because it doesn't require such heavy editing as the other stuff and it feels more natural in a sense as well because I'm just going with the flow. It's difficult to do. I enjoyed this session anyway and if anything, if no one watches, I got a lot of out of reading this paper and having to say my thoughts out loud.

Anyway, all right. Thank you. Until next time.
